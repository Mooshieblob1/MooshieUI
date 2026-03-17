---
name: add-generation-param
description: Add a new generation parameter across the full Svelte → Rust stack
argument-hint: "Describe the parameter: name, type, default value, and which workflow templates use it"
agent: agent
---

Add a new generation parameter to MooshieUI. This requires changes across 6 files in a specific order.

## Required Information

Ask the user for:
1. **Parameter name** (camelCase for TypeScript, snake_case for Rust)
2. **Type** (string, number, boolean, optional/nullable)
3. **Default value**
4. **Which templates use it** (txt2img, img2img, inpainting, upscale, or all)
5. **How it's used in the workflow** (which ComfyUI node and input field)

## Checklist — Edit These Files in Order

### 1. TypeScript interface — `src/lib/types/index.ts`
Add the field to the `GenerationParams` interface (snake_case, matching Rust naming):
```typescript
export interface GenerationParams {
  // ... existing fields ...
  new_param: type;  // snake_case
}
```

### 2. Svelte store field — `src/lib/stores/generation.svelte.ts`
Add a `$state` field to `GenerationStore` (camelCase):
```typescript
newParam = $state<Type>(defaultValue);
```

### 3. Persistence — same file, `loadSettings()` and `saveSettings()`
In `loadSettings()`, add a guarded restore (use `!== undefined` for booleans/numbers that can be 0/false):
```typescript
if (saved.newParam !== undefined) this.newParam = saved.newParam;
```
In `saveSettings()`, add the field to the object passed to `this._store.set()`:
```typescript
newParam: this.newParam,
```

### 4. Params mapping — same file, `toParams()`
Add the camelCase → snake_case mapping:
```typescript
new_param: this.newParam,
```

### 5. Rust struct — `src-tauri/src/comfyui/types.rs`
Add the field to `GenerationParams` (snake_case, serde handles JSON deserialization):
```rust
pub new_param: Type,
// or for optional:
pub new_param: Option<Type>,
```

### 6. Workflow template — `src-tauri/src/templates/{mode}.rs`
Use the parameter in the relevant workflow template's node JSON:
```rust
"node_input_name": params.new_param,
```

## Verification

After all edits, confirm:
- [ ] TypeScript interface has the snake_case field
- [ ] Store has the camelCase `$state` field with correct default
- [ ] `loadSettings()` restores it (with proper null/falsy guard)
- [ ] `saveSettings()` includes it
- [ ] `toParams()` maps camelCase → snake_case
- [ ] Rust `GenerationParams` has the matching snake_case field
- [ ] At least one template uses the parameter
- [ ] `cargo check` passes in `src-tauri/` (type check the Rust side)
