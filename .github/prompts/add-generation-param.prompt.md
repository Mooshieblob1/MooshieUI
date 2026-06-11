---
name: add-generation-param
description: Add a new generation parameter across the full Svelte → Rust stack
argument-hint: "Describe the parameter: name, type, default value, and which workflow templates use it"
agent: agent
---


# Add Generation Parameter (MooshieUI)

**6 touchpoints**, in order. New workflow-only params still need Rust + TS types if exposed in UI.

## Gather from user

1. Name (TS `camelCase`, Rust/iface `snake_case`)
2. Type + default
3. Templates: txt2img | img2img | inpainting | upscale | all
4. ComfyUI node + input field name

## 1. `src/lib/types/index.ts`

```typescript
export interface GenerationParams {
  new_param: SomeType;
}
```

## 2–4. `src/lib/stores/generation.svelte.ts`

```typescript
newParam = $state<SomeType>(defaultValue);

// loadSettings()
if (saved.newParam !== undefined) this.newParam = saved.newParam;

// saveSettings()
newParam: this.newParam,

// toParams()
new_param: this.newParam,
```

Use `!== undefined` for bool/number defaults that can be `0` or `false`.

## 5. `src-tauri/src/comfyui/types.rs`

```rust
pub new_param: Type,
// or
pub new_param: Option<Type>,
```

## 6. `src-tauri/src/templates/{mode}.rs`

```rust
"node_input": params.new_param,
```

## Verify

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

```
- [ ] Interface snake_case field
- [ ] Store $state + load/save + toParams()
- [ ] Rust GenerationParams field
- [ ] ≥1 template references param
```

If UI needs i18n: add keys to **all** files in `src/lib/locales/` (parity with `en.ts`).

For new workflow **modes**, use **workflow-template-builder** skill after params exist.
