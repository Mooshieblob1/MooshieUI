---
applyTo: "src-tauri/**"
---

# Rust / Tauri Backend Instructions

## Command Signatures

Every Tauri command follows this pattern:

```rust
#[tauri::command]
pub async fn command_name(
    state: State<'_, AppState>,       // Always present
    app_handle: AppHandle,            // Only if emitting events or spawning tasks
    param1: String,                   // Frontend params use camelCase → Rust snake_case auto-convert
) -> Result<ReturnType, AppError> {
    // ...
}
```

- Return `Result<T, AppError>` — never panic, never return raw strings as errors.
- Use `State<'_, AppState>` (not `tauri::State`) — it's re-exported.
- Add `AppHandle` only when you need `app.emit()` or `app.path()`.

## Registering New Commands

1. Add the function in the appropriate `commands/*.rs` module (or create a new module).
2. If new module: add `pub mod new_module;` in `commands/mod.rs`.
3. Register in `lib.rs` inside `tauri::generate_handler![...]` — order doesn't matter but group by module.

## State Access

```rust
// Read config (RwLock — use .read().await, hold briefly)
let config = state.config.read().await;
let url = format!("{}/endpoint", config.server_url);
drop(config); // Drop before await-ing network calls

// Write config (RwLock — use .write().await)
let mut config = state.config.write().await;
config.some_field = new_value;
config.save()?;

// Access process (Mutex — use .lock().await)
let mut process = state.comfyui_process.lock().await;
if let Some(ref mut child) = *process {
    child.kill().await.ok();
    *process = None;
}

// Direct access (no lock needed)
let client_id = state.client_id.clone();
let response = state.http_client.get(&url).send().await?;
```

**Rules:**
- Drop `RwLock` guards before `.await` on I/O to avoid holding locks across await points.
- Use `state.http_client` for all HTTP requests — it's connection-pooled.
- Never create a new `reqwest::Client` per request.

## Error Handling

Use the `AppError` enum from `error.rs`. Key variants:

| Variant | Use When |
|---------|----------|
| `ConnectionFailed(String)` | Can't reach ComfyUI server |
| `ProcessSpawnFailed(String)` | ComfyUI process won't start |
| `ApiError { status, message }` | ComfyUI API returned an error |
| `WebSocketError(String)` | WebSocket connection issues |
| `InvalidWorkflow(String)` | Workflow template building fails |
| `IoError` | File system operations (auto-converts via `#[from]`) |
| `SerializationError` | JSON parse/serialize (auto-converts via `#[from]`) |
| `HttpError` | reqwest errors (auto-converts via `#[from]`) |
| `Other(String)` | Anything else |

- Use `?` operator for types with `#[from]` conversion (io::Error, serde_json::Error, reqwest::Error).
- For custom messages: `AppError::Other("descriptive message".into())`.
- `AppError` implements `Serialize` by converting to the Display string — frontend receives error as a string.

## Workflow Templates

Templates live in `src-tauri/src/templates/`. Each template builds a `serde_json::Map<String, Value>` representing a ComfyUI workflow.

### Node ID Pattern

```rust
let mut next_id: u32 = 1;

let checkpoint_id = next_id.to_string();
workflow.insert(checkpoint_id.clone(), json!({
    "class_type": "CheckpointLoaderSimple",
    "inputs": {
        "ckpt_name": params.checkpoint
    }
}));
next_id += 1;
```

- Node IDs are **string numbers** (`"1"`, `"2"`, `"3"`, ...).
- Use `next_id: u32` counter, convert with `.to_string()`, increment after each node.
- Clone the ID string when you need to reference it later in connections.

### Node Connections

Connections use `[node_id_string, output_index]` arrays:

```rust
let sampler_id = next_id.to_string();
workflow.insert(sampler_id.clone(), json!({
    "class_type": "KSampler",
    "inputs": {
        "model": [model_source.0, model_source.1],    // (String, u32) tuple
        "positive": [pos_id.clone(), 0],
        "negative": [neg_id.clone(), 0],
        "latent_image": [latent_id.clone(), 0],
        "seed": params.seed,
        "steps": params.steps,
        "cfg": params.cfg,
        "sampler_name": params.sampler_name,
        "scheduler": params.scheduler,
        "denoise": 1.0
    }
}));
next_id += 1;
```

- Track connection sources as `(String, u32)` tuples — (source node ID, output port index).
- Port 0 is the first output of a node. Most nodes have one output (port 0).

### Adding a New Template

1. Create `src-tauri/src/templates/new_mode.rs` with a `pub fn build(params: &GenerationParams) -> Result<Map<String, Value>, AppError>`.
2. Add `pub mod new_mode;` in `templates/mod.rs`.
3. Add a match arm in `templates/mod.rs::build_workflow()` for the new mode string.
4. The template receives `&GenerationParams` — access fields directly.
5. Return the workflow map. The `SaveImage` node is added by `mod.rs`, not individual templates.

### LoRA Chaining Pattern

```rust
// Track the current model and clip sources
let mut model_source = (checkpoint_id.clone(), 0u32);
let mut clip_source = (checkpoint_id.clone(), 1u32);

for lora in &params.loras {
    let lora_id = next_id.to_string();
    workflow.insert(lora_id.clone(), json!({
        "class_type": "LoraLoader",
        "inputs": {
            "model": [model_source.0, model_source.1],
            "clip": [clip_source.0, clip_source.1],
            "lora_name": lora.name,
            "strength_model": lora.strength_model,
            "strength_clip": lora.strength_clip
        }
    }));
    model_source = (lora_id.clone(), 0);
    clip_source = (lora_id.clone(), 1);
    next_id += 1;
}
```

## Emitting Events to Frontend

```rust
// In a command with AppHandle
app.emit("comfyui:my_event", json!({ "key": "value" }))?;

// Event naming convention: "comfyui:{event_type}" for ComfyUI-related events
// "setup:{event_type}" for setup-related events
```

Frontend listens via `listen("comfyui:my_event", callback)` in `App.svelte`.

## Module Organization

- `commands/` — Tauri command handlers only. No business logic. Delegate to `comfyui/` modules.
- `comfyui/` — Core integration: HTTP client (`client.rs`), process management (`process.rs`), WebSocket (`websocket.rs`), types (`types.rs`).
- `templates/` — Workflow JSON builders. Pure functions that take `&GenerationParams` and return workflow maps.
- `state.rs` — `AppState` struct. Shared across all commands via Tauri's managed state.
- `config.rs` — `AppConfig` with JSON persistence. Platform-aware paths via `dirs` crate.
- `error.rs` — `AppError` enum. Single error type for all commands.
- `setup.rs` — One-click installer. Self-contained; uses its own error handling (`Result<(), String>`).

## Key Dependencies

| Crate | Usage |
|-------|-------|
| `tokio` (full) | Async runtime, process spawning, sleep/delays |
| `reqwest` 0.12 | HTTP client — use `state.http_client` |
| `tokio-tungstenite` 0.26 | WebSocket client for ComfyUI streaming |
| `serde` / `serde_json` | All serialization. Use `json!{}` macro for workflow building |
| `thiserror` 2 | Derive `Error` for `AppError` |
| `dirs` 5 | Platform config/data paths |
| `uuid` 1 | Client ID generation (v4) |
| `base64` 0.22 | Encode binary WebSocket data for frontend events |
