---
name: add-tauri-command
description: Add a new Tauri command with Rust handler, TypeScript wrapper, and registration
argument-hint: "Describe the command: name, parameters, return type, and what it does"
agent: agent
---


# Add Tauri Command (MooshieUI)

Minimum **3 files**, in this order. Works in **desktop and browser mode** only if you use `ipcInvoke()` (never raw `invoke()`).

## Gather from user

1. Command name (Rust `snake_case`, TS `camelCase` function)
2. Parameters (name + type each)
3. Return type
4. Module: `api` | `server` | `websocket` | `workflow` | `config` | new
5. Needs `AppHandle`? (only for events / app paths)

## 1. Rust — `src-tauri/src/commands/{module}.rs`

```rust
#[tauri::command]
pub async fn my_command(
    state: State<'_, AppState>,
    my_param: String,
) -> Result<ReturnType, AppError> {
    let config = state.config.read().await;
    // ...
    drop(config);
    Ok(result)
}
```

- `Result<T, AppError>` only; no panic
- `AppHandle` only if emitting events / spawning tasks
- Drop `RwLock` before I/O `.await`
- HTTP via `state.http_client`
- New module → `pub mod x;` in `commands/mod.rs`

## 2. Register — `src-tauri/src/lib.rs`

```rust
.invoke_handler(tauri::generate_handler![
    commands::api::my_command,
])
```

## 3. TypeScript — `src/lib/utils/api.ts`

```typescript
export async function myCommand(myParam: string): Promise<ReturnType> {
  return ipcInvoke("my_command", { myParam });
}
```

- camelCase keys in payload; Rust receives snake_case
- New shapes → `src/lib/types/index.ts` (serde field names, often snake_case)

## Verify

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

```
- [ ] #[tauri::command] + Result<_, AppError>
- [ ] Listed in generate_handler!
- [ ] api.ts uses ipcInvoke("my_command", ...)
- [ ] Names aligned TS camelCase ↔ Rust snake_case
```
