---
applyTo: "src/lib/stores/**"
---

# Svelte 5 Rune Store Conventions

## Store Structure

Every store is a **class with `$state` rune fields**, exported as a **singleton instance**:

```typescript
class MyStore {
  fieldName = $state<Type>(defaultValue);
  // ...methods...
}

export const myStore = new MyStore();
```

**Rules:**
- File extension: `*.svelte.ts` (required for rune compilation).
- One store class per file, one exported singleton.
- Do NOT use legacy Svelte stores (`writable`, `readable`, `derived`).
- Export name matches the store's domain: `connection`, `models`, `generation`, `progress`, `gallery`.

## Reactivity

- **Mutable fields**: Use `$state<Type>(default)` — reactivity is automatic on assignment.
- **Computed values**: Use native `get` accessors (not `$derived`):
  ```typescript
  get percentage() {
    return this.totalSteps > 0 ? (this.currentStep / this.totalSteps) * 100 : 0;
  }
  ```
- **Array mutations**: Always reassign (spread) — don't use `.push()`:
  ```typescript
  // ✅ Correct — triggers reactivity
  this.images = [...newImages, ...this.images];
  this.loras = this.loras.filter((_, i) => i !== index);

  // ❌ Wrong — won't trigger reactivity
  this.images.push(newImage);
  ```

## Persistence

Only `generation.svelte.ts` persists settings. The pattern uses `@tauri-apps/plugin-store`:

```typescript
import { load } from "@tauri-apps/plugin-store";

const STORE_KEY = "my-settings";

class MyStore {
  field = $state("default");
  private _store: Awaited<ReturnType<typeof load>> | null = null;

  async loadSettings() {
    this._store = await load("settings.json", { autoSave: true });
    const saved = await this._store.get<Record<string, any>>(STORE_KEY);
    if (saved) {
      if (saved.field) this.field = saved.field;
    }
  }

  async saveSettings() {
    if (!this._store) return;
    await this._store.set(STORE_KEY, { field: this.field });
  }
}
```

**Rules:**
- `saveSettings()` must be called **explicitly** after mutations. It does NOT auto-save on field change.
- Use `autoSave: true` in the `load()` call so the Tauri store flushes to disk.
- Guard nullable fields with `!== undefined` checks (not just truthiness) when loading — `0`, `false`, `""` are valid saved values.
- Wrap both `loadSettings()` and `saveSettings()` in try/catch — persistence failure should not break the UI.

## API Integration

- Stores call Tauri backend through `src/lib/utils/api.ts` wrappers — never call `invoke()` directly.
- Use `Promise.all()` for parallel fetches (see `models.svelte.ts::refresh()`).
- Set a `loading = $state(false)` field for async operations, reset in `finally` block.

## The `toParams()` Pattern

`generation.svelte.ts` has a `toParams()` method that converts camelCase store fields to snake_case for the Rust backend:

```typescript
toParams() {
  return {
    positive_prompt: this.positivePrompt,  // camelCase → snake_case
    sampler_name: this.samplerName,
    batch_size: this.batchSize,
    // ... all fields mapped
  };
}
```

When adding new generation parameters:
1. Add the `$state` field (camelCase).
2. Add it to `loadSettings()` and `saveSettings()`.
3. Add the snake_case mapping in `toParams()`.
4. Add the corresponding field to the Rust `GenerationParams` struct.

## Error Handling

- Use `try/catch` with `console.error()` — no toast/alert library exists.
- Never throw from store methods — swallow and log errors so the UI stays functional.
- The `gallery` store has its own `showToast()` pattern for user-facing messages.

## Store Interaction

- Stores do NOT import each other directly.
- Cross-store coordination happens in `App.svelte` (e.g., loading models → applying defaults to generation).
- Keep stores independent and focused on a single domain.
