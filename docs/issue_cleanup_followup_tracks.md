# Issue Cleanup Follow-Up Tracks

This note separates the unrelated branch work from the issue-focused fixes for `#194`, `#196`, `#197`, `#198`, and the first `#199` slice.

## 1. Platform And Browser/Server Hardening

Use this track for runtime stability, launch behavior, worker orchestration, and browser/server-mode coordination.

- `src-tauri/src/comfyui/gpu_manager.rs`
- `src-tauri/src/comfyui/process.rs`
- `src-tauri/src/webserver.rs`
- `src/lib/components/settings/SettingsPage.svelte`

## 2. Regional Prompting Hardening

Use this track for syntax safety, UI polish, workflow correctness, and generation-store integration around regional prompts.

- `src/lib/components/generation/RegionalPromptModal.svelte`
- `src/lib/stores/generation.svelte.ts`
- `src/lib/utils/promptSchedule.ts`
- `src/lib/utils/regionalInpaintChain.ts`
- `src-tauri/src/templates/txt2img.rs`

## 3. Style Transfer Hardening

Use this track for Anima Untwisting RoPE UX, workflow validation, and node/runtime error handling.

- `src/lib/components/generation/StyleTransferSettings.svelte`
- `src-tauri/src/templates/style_transfer.rs`
- `src-tauri/src/comfyui/nodes.rs`

## 4. Output Automation Polish

Use this track for filename templating, webhook delivery, and related persistence/runtime behavior.

- `src-tauri/src/config.rs`
- `src-tauri/src/commands/api.rs`
- `src-tauri/src/webserver.rs`
