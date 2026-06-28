# M2 — Parameter Correctness & Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Illustrious generate stably at the correct low CFG, make CFG=1 impossible to footgun silently, add an opt-in Advanced Mode that stops auto-tuning generation params on checkpoint change, and stop the upscale pass from collapsing CFG++ samplers at low base CFG.

**Architecture:** Four isolated changes. (1) The Illustrious non-turbo preset switches from `euler_cfg_pp` @ 5.0 to `euler_ancestral_cfg_pp` @ 2.0 with a graceful sampler fallback; (2) the sampler panel raises the cfg_pp recommended floor and adds a prominent CFG=1 warning box; (3) a new persisted `advancedMode` flag gates the param-writing half of `applyModelSpecificPreset()`; (4) the Rust upscale template floors CFG at 2.0 for CFG++ samplers. No new Tauri commands, no guidance-node changes.

**Tech Stack:** Svelte 5 runes (class-singleton store in `*.svelte.ts`), TypeScript, Tauri/Rust template builders, flat-key i18n in `src/lib/locales/*.ts`.

**Spec:** `docs/superpowers/specs/2026-06-17-m2-parameter-correctness-control-design.md`

**Project-specific constraints (apply to every task):**
- **No test framework exists.** "Tests" are the build gates: `npm run build` (PASS = output ends with `✓ built in`) for frontend/TS/Svelte changes, and `cargo check --manifest-path src-tauri/Cargo.toml` for Rust changes. Plus the manual verification noted per task.
- **Windows git:** prefix every git command with `git -c core.hooksPath=/dev/null` (the bash pre-commit hook hangs in PowerShell).
- **No `Co-Authored-By` trailers** in any commit message.
- **i18n parity is enforced:** every key added to `src/lib/locales/en.ts` must be added to all 10 other locale files (`de, es, fr, it, ja, ko, pt, ru, zh, zh-tw`). Locale files use a **flat** key structure (`"a.b.c": "value"`), not nested objects. English fallback text in the non-English files is acceptable per existing repo precedent.
- **Svelte/Tailwind rules:** Tailwind classes only (no `<style>` blocks); `onclick`/`onchange` not legacy `on:click`; dark-neutral palette.

---

## Task 1: Fix the Illustrious non-turbo preset + sampler fallback

**Files:**
- Modify: `src/lib/stores/generation.svelte.ts` (the `ModelPreset` interface ~line 49; `applyResolvedPreset()` ~line 844; the `case "illustrious"` block ~lines 1069-1078)

**Context:** `applyResolvedPreset()` already routes the preset sampler through `resolveAvailableOption(models.samplers, preset.samplerName, "euler")`, which falls back to the literal `"euler"` if the preferred sampler is absent from the backend's enumerated sampler list. For Illustrious we want the fallback to be `euler_ancestral` (preserve the ancestral character), so we add an optional per-preset fallback field. Only the `illustrious` case changes — the `sdxl`/`mugen`/`unknown`/default block keeps `euler_cfg_pp` @ 5.0, and Anima/Wan/Qwen keep `er_sde` @ 4.0.

- [ ] **Step 1: Add an optional `samplerFallback` field to the `ModelPreset` interface**

In `src/lib/stores/generation.svelte.ts`, the interface currently reads (lines 49-57):

```ts
interface ModelPreset {
  steps: number;
  cfg: number;
  samplerName: string;
  scheduler: string;
  width: number;
  height: number;
  upscaleDenoise?: number;
}
```

Add the field:

```ts
interface ModelPreset {
  steps: number;
  cfg: number;
  samplerName: string;
  scheduler: string;
  width: number;
  height: number;
  upscaleDenoise?: number;
  /** Sampler to use when `samplerName` is absent from the backend's enumerated list. Defaults to "euler". */
  samplerFallback?: string;
}
```

- [ ] **Step 2: Use `samplerFallback` in `applyResolvedPreset()`**

The line is currently (line 844):

```ts
    this.samplerName = this.resolveAvailableOption(models.samplers, preset.samplerName, "euler");
```

Change it to:

```ts
    this.samplerName = this.resolveAvailableOption(models.samplers, preset.samplerName, preset.samplerFallback ?? "euler");
```

Leave the scheduler line (845) unchanged.

- [ ] **Step 3: Update the `case "illustrious"` block**

The block is currently (lines 1069-1078):

```ts
      case "illustrious":
        preset = {
          steps: this.hasTurboModelVariant ? 10 : 20,
          cfg: this.hasTurboModelVariant ? 1.0 : 5.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "euler_cfg_pp",
          scheduler: this.hasTurboModelVariant ? "normal" : "sgm_uniform",
          width: 1024,
          height: 1024,
        };
        break;
```

Replace it with:

```ts
      case "illustrious":
        preset = {
          steps: this.hasTurboModelVariant ? 10 : 20,
          // euler_ancestral_cfg_pp is a CFG++ sampler tuned for low CFG (~1.5-2.2);
          // CFG 2.0 keeps it inside its band. Falls back to plain euler_ancestral
          // on older ComfyUI builds that lack the cfg_pp variant.
          cfg: this.hasTurboModelVariant ? 1.0 : 2.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "euler_ancestral_cfg_pp",
          samplerFallback: this.hasTurboModelVariant ? "euler" : "euler_ancestral",
          scheduler: this.hasTurboModelVariant ? "normal" : "sgm_uniform",
          width: 1024,
          height: 1024,
        };
        break;
```

Do **not** touch the `case "sdxl": case "mugen": case "unknown": default:` block (lines 1094-1106) or any other case.

- [ ] **Step 4: Verify the build passes**

Run: `npm run build`
Expected: output ends with `✓ built in` (no TS errors about `samplerFallback`).

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/stores/generation.svelte.ts
git -c core.hooksPath=/dev/null commit -m "fix: correct Illustrious non-turbo preset to euler_ancestral_cfg_pp @ CFG 2.0"
```

**Manual verification (note for later, not a blocker):** Selecting a non-turbo Illustrious checkpoint should set sampler `euler_ancestral_cfg_pp` (or `euler_ancestral` on older backends) and CFG 2.0.

---

## Task 2: CFG=1 loud warning + fix the misleading cfg_pp range

**Files:**
- Modify: `src/lib/components/generation/SamplerSettings.svelte` (`recommendedCfgRange()` ~line 32-35; insert a warning box after the recommendations row ~line 245)
- Modify: `src/lib/locales/en.ts` (add 2 keys after the existing `generation.sampler.*` block)
- Modify: all 10 other locale files (`src/lib/locales/{de,es,fr,it,ja,ko,pt,ru,zh,zh-tw}.ts`) — add the same 2 keys

**Context:** `recommendedCfgRange()` currently treats CFG 1.0 as in-range for cfg_pp samplers (`min: 1.0`), so the UI never warns at the exact value that breaks CFG++ samplers. We raise the floor and add a prominent, non-blocking warning box that only appears at `cfg <= 1.0`. The existing small amber recommendations row (lines 230-245) stays for the milder out-of-range case.

- [ ] **Step 1: Raise the cfg_pp recommended floor**

In `src/lib/components/generation/SamplerSettings.svelte`, the function is currently (lines 32-35):

```ts
  function recommendedCfgRange() {
    if (isCfgPpSampler(generation.samplerName)) return { min: 1.0, max: 2.2, target: 1.4 };
    return { min: 4.0, max: 8.0, target: 6.0 };
  }
```

Change only the cfg_pp branch:

```ts
  function recommendedCfgRange() {
    if (isCfgPpSampler(generation.samplerName)) return { min: 1.5, max: 2.2, target: 1.8 };
    return { min: 4.0, max: 8.0, target: 6.0 };
  }
```

- [ ] **Step 2: Add the i18n keys to `en.ts`**

In `src/lib/locales/en.ts`, find the `generation.sampler.juice_hint` line (currently line 589). Immediately after it, add:

```ts
  "generation.sampler.cfg1_warning_title": "CFG 1 disables prompt guidance",
  "generation.sampler.cfg1_warning_body": "At CFG 1 the model ignores your prompt's guidance. This only produces good results on Turbo, distilled, or Lightning models, and it breaks CFG++ samplers (like euler_cfg_pp / euler_ancestral_cfg_pp) entirely. Raise CFG to the recommended range unless you know your model needs CFG 1.",
```

- [ ] **Step 3: Add the same 2 keys to all 10 other locale files**

For each of `src/lib/locales/de.ts`, `es.ts`, `fr.ts`, `it.ts`, `ja.ts`, `ko.ts`, `pt.ts`, `ru.ts`, `zh.ts`, `zh-tw.ts`: locate the `"generation.sampler.juice_hint":` line in that file and add the same two lines immediately after it (the English text above is acceptable as the value in every file — do not invent translations):

```ts
  "generation.sampler.cfg1_warning_title": "CFG 1 disables prompt guidance",
  "generation.sampler.cfg1_warning_body": "At CFG 1 the model ignores your prompt's guidance. This only produces good results on Turbo, distilled, or Lightning models, and it breaks CFG++ samplers (like euler_cfg_pp / euler_ancestral_cfg_pp) entirely. Raise CFG to the recommended range unless you know your model needs CFG 1.",
```

Note: these keys have **no `{placeholder}`** interpolation, so there is nothing to keep in sync beyond the key names themselves.

- [ ] **Step 4: Add the prominent warning box after the recommendations row**

In `SamplerSettings.svelte`, the recommendations row closes at line 245 with `</div>`, immediately followed by the `<!-- Seed + Batch Size -->` comment at line 247. Insert the warning box between them (after line 245, before line 247):

```svelte
  {#if generation.cfg <= 1.0}
    <div class="flex items-start gap-2 rounded-lg border border-amber-600/60 bg-amber-950/30 px-3 py-2 mt-1">
      <svg class="w-4 h-4 mt-0.5 shrink-0 text-amber-400" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
      <div>
        <p class="text-xs font-semibold text-amber-200">{locale.t('generation.sampler.cfg1_warning_title')}</p>
        <p class="text-[10px] text-amber-300/80 mt-0.5">{locale.t('generation.sampler.cfg1_warning_body')}</p>
      </div>
    </div>
  {/if}
```

This is non-blocking (display only). `locale` is already imported in this component (used throughout).

- [ ] **Step 5: Verify the build passes**

Run: `npm run build`
Expected: output ends with `✓ built in`.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/generation/SamplerSettings.svelte src/lib/locales
git -c core.hooksPath=/dev/null commit -m "feat: prominent CFG=1 warning and corrected cfg_pp recommended range"
```

**Manual verification (note for later):** Drag CFG to 1.0 → the prominent amber warning box appears; the cfg_pp recommended-range text reads `1.5-2.2`; generation still proceeds.

---

## Task 3: Add the `advancedMode` flag and gate `applyModelSpecificPreset()`

**Files:**
- Modify: `src/lib/stores/generation.svelte.ts` (declare flag ~line 359; gate inside `applyModelSpecificPreset()` ~line 868; load guard ~line 1219; two save objects ~line 1346 and ~line 1438)

**Context:** `advancedMode` is a persisted app-level preference (mirror the `manualSaveMode` wiring exactly — declaration, one load guard, two save objects). When on, `applyModelSpecificPreset()` still runs family detection and updates `modelPresetAppliedKey` (so template behavior stays correct), but skips writing the user-facing params (`steps`, `cfg`, `samplerName`, `scheduler`, `width`, `height`) — which all happen inside `applyResolvedPreset()`. A first-ever application is exempt so fresh users still get sane defaults. Family/architecture metadata is set in `applyModelMetadata()` (a separate method), so skipping the preset application here does not affect family detection.

- [ ] **Step 1: Declare the flag**

In `src/lib/stores/generation.svelte.ts`, after the `autoSaveDirs` declaration (line 361):

```ts
  autoSaveDirs = $state<string[]>([]);
```

add:

```ts
  /** When true, swapping checkpoints no longer auto-applies per-model generation
   *  presets (steps/cfg/sampler/scheduler/dimensions) — power users keep their tuning.
   *  The first preset of a fresh profile is still applied so defaults aren't nonsense. */
  advancedMode = $state(false);
```

- [ ] **Step 2: Gate the preset application**

In `applyModelSpecificPreset()`, the idempotency guard is currently (lines 867-868):

```ts
    if (presetKey === this.modelPresetAppliedKey) return;
    this.modelPresetAppliedKey = presetKey;
```

Replace with:

```ts
    if (presetKey === this.modelPresetAppliedKey) return;
    const isFirstPresetApplication = !this.modelPresetAppliedKey;
    this.modelPresetAppliedKey = presetKey;

    // Advanced Mode: once a preset has been applied at least once, preserve the
    // user's generation params across checkpoint swaps. Family/architecture
    // metadata is set in applyModelMetadata(), so templates still behave correctly.
    if (this.advancedMode && !isFirstPresetApplication) return;
```

- [ ] **Step 3: Restore the flag in `loadSettings()`**

Find the `manualSaveMode` load guard (line 1219):

```ts
        if (saved.manualSaveMode !== undefined) this.manualSaveMode = saved.manualSaveMode;
```

Add immediately after it:

```ts
        if (saved.advancedMode !== undefined) this.advancedMode = saved.advancedMode;
```

- [ ] **Step 4: Persist the flag in both save objects**

First save object — find (line 1346):

```ts
        manualSaveMode: this.manualSaveMode,
```

Add immediately after:

```ts
        advancedMode: this.advancedMode,
```

Second save object — find (line 1438, note the shallower indentation here):

```ts
      manualSaveMode: this.manualSaveMode,
```

Add immediately after:

```ts
      advancedMode: this.advancedMode,
```

- [ ] **Step 5: Verify the build passes**

Run: `npm run build`
Expected: output ends with `✓ built in`.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/stores/generation.svelte.ts
git -c core.hooksPath=/dev/null commit -m "feat: add advancedMode flag gating per-model preset auto-apply"
```

**Manual verification deferred to Task 4** (needs the toggle UI to set the flag).

---

## Task 4: Advanced Mode toggle UI + confirmation modal

**Files:**
- Modify: `src/lib/components/settings/SettingsPage.svelte` (add a `showAdvancedModeWarning` state var in the `<script>`; add the toggle inside the `quality` section ~line 2567; add the confirmation modal near the `showQualityTagsWarning` modal ~line 4175)
- Modify: `src/lib/locales/en.ts` (add 6 keys after the `settings.quality_warning.*` block)
- Modify: all 10 other locale files — add the same 6 keys

**Context:** Mirror the existing `autoQualityTags` confirmation pattern: the checkbox reflects the store value; enabling it pops a confirmation modal (the box doesn't flip until confirmed); disabling is silent. The toggle lives in the `quality` settings section (its keyword list already includes "illustrious pony" model concepts). The `showQualityTagsWarning` modal at lines 4149-4175 is the exact template to copy.

- [ ] **Step 1: Add the modal state variable**

In the `<script>` of `src/lib/components/settings/SettingsPage.svelte`, find the existing `showQualityTagsWarning` declaration (search for `showQualityTagsWarning`) and add a sibling declaration next to it:

```ts
  let showAdvancedModeWarning = $state(false);
```

(Match the existing declaration style in that file — if `showQualityTagsWarning` uses `let showQualityTagsWarning = $state(false);`, mirror it exactly.)

- [ ] **Step 2: Add the toggle inside the quality section**

In the quality section, the auto-quality-tags toggle block ends at line 2567 with `</div>`, immediately before `{#if generation.autoQualityTags}` at line 2569. Insert the Advanced Mode toggle between them (after line 2567, before line 2569):

```svelte
          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="advanced-mode"
              checked={generation.advancedMode}
              onchange={(e) => {
                const target = e.target as HTMLInputElement;
                if (target.checked) {
                  // Revert visually — let the confirmation popup decide.
                  target.checked = false;
                  showAdvancedModeWarning = true;
                } else {
                  generation.advancedMode = false;
                  generation.saveSettings();
                }
              }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="advanced-mode" class="text-sm text-neutral-200">{locale.t('settings.advanced_mode.label')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.advanced_mode.desc')}</p>
            </div>
          </div>
```

- [ ] **Step 3: Add the confirmation modal**

After the `showQualityTagsWarning` modal's closing `{/if}` (line 4175), add:

```svelte
{#if showAdvancedModeWarning}
<div class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center" role="dialog">
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl p-6 max-w-md mx-4 shadow-2xl">
    <h3 class="text-sm font-semibold text-neutral-100 mb-3">{locale.t('settings.advanced_mode.warning_title')}</h3>
    <p class="text-xs text-neutral-400 mb-4">{locale.t('settings.advanced_mode.warning_body')}</p>
    <div class="flex gap-3 justify-end">
      <button
        onclick={() => { showAdvancedModeWarning = false; }}
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
      >
        {locale.t('settings.advanced_mode.cancel')}
      </button>
      <button
        onclick={() => {
          generation.advancedMode = true;
          generation.saveSettings();
          showAdvancedModeWarning = false;
        }}
        class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-xs font-medium transition-colors cursor-pointer"
      >
        {locale.t('settings.advanced_mode.enable')}
      </button>
    </div>
  </div>
</div>
{/if}
```

- [ ] **Step 4: Add the 5 i18n keys to `en.ts`**

In `src/lib/locales/en.ts`, find the `settings.quality_warning.disable` line (line 454). Immediately after it, add:

```ts
  "settings.advanced_mode.label": "Advanced mode (lock generation parameters)",
  "settings.advanced_mode.desc": "When on, switching checkpoints no longer auto-adjusts steps, CFG, sampler, scheduler, or dimensions. Your manual settings are preserved.",
  "settings.advanced_mode.warning_title": "Enable advanced mode?",
  "settings.advanced_mode.warning_body": "Steps, CFG, sampler, scheduler, and dimensions will stop auto-tuning when you change models. Recommended defaults will no longer be applied, so make sure your settings suit each model you load.",
  "settings.advanced_mode.enable": "Enable",
  "settings.advanced_mode.cancel": "Cancel",
```

- [ ] **Step 5: Add the same 6 keys to all 10 other locale files**

For each of `de.ts, es.ts, fr.ts, it.ts, ja.ts, ko.ts, pt.ts, ru.ts, zh.ts, zh-tw.ts`: find the `"settings.quality_warning.disable":` line and add the same six lines from Step 4 immediately after it (English text acceptable as the value). None of these keys use `{placeholder}` interpolation.

- [ ] **Step 6: Verify the build passes**

Run: `npm run build`
Expected: output ends with `✓ built in`.

- [ ] **Step 7: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/settings/SettingsPage.svelte src/lib/locales
git -c core.hooksPath=/dev/null commit -m "feat: Advanced Mode toggle with confirmation dialog in settings"
```

**Manual verification (note for later):** Enable Advanced Mode (confirm the dialog) → set a custom CFG/sampler/steps/dimensions → swap checkpoints → those params are preserved. Disable Advanced Mode → swapping checkpoints auto-applies per-model presets again. A brand-new profile with no prior preset still gets defaults on first model select.

---

## Task 5: Upscale CFG floor for CFG++ samplers (Rust)

**Files:**
- Modify: `src-tauri/src/templates/upscale.rs` (the second-pass KSampler ~lines 191-210)

**Context:** The upscale pass halves the base CFG (`params.cfg / 2.0`) and inherits the base sampler. With the new Illustrious default (CFG 2.0 + a CFG++ sampler), this would produce CFG 1.0 on `euler_ancestral_cfg_pp` — the exact collapse this milestone fixes. Floor the upscale CFG at 2.0 when the sampler is a CFG++ variant. The detection idiom (`sampler_name.to_lowercase().contains("cfg_pp")`) mirrors `inpainting.rs:113`. Compute the values *before* the `json!` block, since `params.sampler_name` is moved into the workflow at line 205.

- [ ] **Step 1: Compute the floored CFG before the KSampler insert**

In `src-tauri/src/templates/upscale.rs`, the second KSampler pass begins at line 191. The block is currently:

```rust
    // Second KSampler pass at low denoise
    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_after_soft.0, model_after_soft.1],
                "positive": [pos_source.0, pos_source.1],
                "negative": [neg_source.0, neg_source.1],
                "latent_image": [latent_source.0.clone(), latent_source.1],
                "seed": seed + 1,
                "steps": params.upscale_steps,
                "cfg": params.cfg / 2.0,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": params.upscale_denoise
            }
        }),
    );
```

Change it to:

```rust
    // Second KSampler pass at low denoise
    let sampler_id = next_id.to_string();
    // CFG++ samplers collapse to unconditional output at CFG=1. Halving a low base
    // CFG (e.g. Illustrious 2.0) would land there, so floor the upscale CFG at 2.0
    // for CFG++ variants; other samplers keep the plain half-CFG behaviour.
    let is_cfgpp_sampler = params.sampler_name.to_lowercase().contains("cfg_pp");
    let upscale_cfg = if is_cfgpp_sampler {
        (params.cfg / 2.0).max(2.0)
    } else {
        params.cfg / 2.0
    };
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_after_soft.0, model_after_soft.1],
                "positive": [pos_source.0, pos_source.1],
                "negative": [neg_source.0, neg_source.1],
                "latent_image": [latent_source.0.clone(), latent_source.1],
                "seed": seed + 1,
                "steps": params.upscale_steps,
                "cfg": upscale_cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": params.upscale_denoise
            }
        }),
    );
```

- [ ] **Step 2: Verify Rust compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: finishes with no errors (a benign unused-warning is not expected since both `is_cfgpp_sampler` and `upscale_cfg` are used).

- [ ] **Step 3: Verify Rust formatting on the changed lines**

Run: `cd src-tauri && cargo fmt --check`
Expected: no diffs that overlap the lines you changed. If `cargo fmt` reports your new lines, run `cargo fmt` and re-stage.

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/templates/upscale.rs
git -c core.hooksPath=/dev/null commit -m "fix: floor upscale CFG at 2.0 for CFG++ samplers"
```

**Manual verification (note for later):** Generate with the new Illustrious default and enable upscale → the upscale pass runs at CFG 2.0 (not 1.0) and produces a clean result.

---

## Final verification (after all tasks)

- [ ] `npm run build` → ends with `✓ built in`
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` → no errors
- [ ] Run the `pre-commit-check` skill once over the full changeset to confirm i18n key parity across all 11 locale files and Svelte/store conventions.
- [ ] Spot-check each "Done when" from the spec (the per-task manual-verification notes above).

This milestone is independently releasable as a stability/correctness release.
