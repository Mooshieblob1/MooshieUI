---
name: add-tauri-command
description: Add a new Tauri command with Rust handler, TypeScript wrapper, and registration
argument-hint: "Describe the command: name, parameters, return type, and what it does"
agent: agent
---

Add a new Tauri command to MooshieUI. This requires changes across 3 files (minimum), always in this order.

## Required Information

Ask the user for:
1. **Command name** (snake_case for Rust, camelCase for TypeScript)
2. **Parameters** (name + type for each)
3. **Return type**
4. **Which command module** it belongs to (`api`, `server`, `websocket`, `workflow`, or new module)
5. **Whether it needs `AppHandle`** (only if emitting events or accessing app paths)

## Checklist — Edit These Files in Order

### 1. Rust command handler — `src-tauri/src/commands/{module}.rs`

Add the command function. Follow this exact pattern:

```rust
#[tauri::command]
pub async fn my_command(
    state: State<'_, AppState>,
    my_param: String,              // Tauri auto-converts camelCase from frontend
) -> Result<ReturnType, AppError> {
    let config = state.config.read().await;
    // ... implementation ...
    Ok(result)
}
```

Rules:
- Always return `Result<T, AppError>` — never panic.
- Use `State<'_, AppState>` for state access.
- Add `app_handle: AppHandle` only if emitting events or spawning background tasks.
- Drop `RwLock` guards before `.await` on I/O operations.
- Use `state.http_client` for HTTP requests (shared, pooled).

If creating a **new module**, also add `pub mod new_module;` in `src-tauri/src/commands/mod.rs`.

### 2. Register in lib.rs — `src-tauri/src/lib.rs`

Add the command to the `generate_handler!` macro, grouped with its module:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    commands::api::my_command,  // ← add here, grouped with module
])
```

### 3. TypeScript wrapper — `src/lib/utils/api.ts`

Add a typed async function that wraps `invoke()`:

```typescript
export async function myCommand(myParam: string): Promise<ReturnType> {
  return invoke("my_command", { myParam });
}
```

Rules:
- Function name: camelCase version of the Rust snake_case name.
- Parameter object keys: camelCase (Tauri auto-converts to snake_case for Rust).
- Add any new types to `src/lib/types/index.ts` and import them.

### 4. (Optional) Add types — `src/lib/types/index.ts`

If the command returns a new shape, add the interface:

```typescript
export interface MyResponse {
  field_name: string;  // Match Rust struct field names (snake_case from serde)
}
```

## Verification

After all edits, confirm:
- [ ] Rust function has `#[tauri::command]` attribute
- [ ] Returns `Result<T, AppError>`
- [ ] Registered in `lib.rs` `generate_handler![]`
- [ ] TypeScript wrapper exists in `api.ts` with correct types
- [ ] Parameter names match between TS (camelCase) and Rust (snake_case)
- [ ] `cargo check` passes in `src-tauri/`
