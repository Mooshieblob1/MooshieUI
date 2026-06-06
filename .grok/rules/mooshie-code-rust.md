---
trigger: glob
description: MooshieUI Rust/Tauri — commands, AppError, templates, RwLock patterns
globs: src-tauri/**/*
---

# Code — Rust (MooshieUI)

Mirrors `.roo/rules-code/AGENTS.md` (Rust) + `.github/instructions/tauri-backend.instructions.md`.

## Commands & State

- `#[tauri::command]` → `Result<T, AppError>`; no panic.
- **State import**: Use `State<'_, AppState>` (re-exported) instead of `tauri::State`.
- **AppHandle**: Add `app_handle: AppHandle` only when emitting events or accessing app paths.
- Register commands in `lib.rs` `generate_handler![]`; TS wrapper in `api.ts` via `ipcInvoke`.
- Drop `RwLock` guards before `.await` on I/O.
- HTTP: `state.http_client` (shared pool). Never create new clients per request.
- **Event Emission**: Event names must follow conventions: `"comfyui:{event_type}"` for ComfyUI-related events and `"setup:{event_type}"` for setup wizard events.

## Error Handling

- All Tauri commands must return `Result<T, AppError>`. Never return raw strings as errors.
- Use `?` operator for auto-convertible types (`io::Error`, `serde_json::Error`, `reqwest::Error`) that map to `AppError` via `#[from]`.
- For custom/custom-handled failures: use `AppError::Other("descriptive text".into())`.

## Templates & Connections

- Node IDs: `next_id.to_string()`; wires as `(String, u32)` → JSON arrays.
- **Node connections**: Track connection sources as `(String, u32)` tuples: (source node ID, output port index).
- **LoRA Chaining Pattern**: Track current model and clip sources (`model_source` and `clip_source`), and thread them through sequential `LoraLoader` nodes.
- Return complete `WorkflowResult` from builders.
- The terminal `SaveImage` node is appended by `templates/mod.rs` rather than by individual template files.

## Related skills

- New command → `add-tauri-command`
- New param → `add-generation-param`
- New mode → `workflow-template-builder`
