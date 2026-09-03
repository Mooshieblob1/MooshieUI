# Bot Review Triage

Updated: 2026-09-04

## Fixed 2026-09-04

The following items from the "Fix Soon" and related sections were addressed in a
single hardening pass. Each entry references the file changed.

- **Default admin password footgun** (`.env.example`, `k8s/secret.yaml`,
  `src-tauri/src/server_main.rs`): `.env.example` no longer ships `changeme`;
  `k8s/secret.yaml` uses an obviously invalid placeholder `REPLACE_ME`;
  `server_main.rs` now refuses to create the admin account when the password
  equals `changeme` (logs an error and skips account creation).

- **INT8-Fast + GGUF silent bypass** (`src-tauri/src/templates/mod.rs`):
  `validate_params` now returns a hard error when INT8-Fast is enabled and the
  diffusion model is a `.gguf` file, instead of silently ignoring the
  incompatible combination.

- **Proxy upload rejection false positives** (`src-tauri/src/comfyui/client.rs`):
  `is_proxy_upload_rejection` heuristic tightened to avoid matching plain
  ComfyUI JSON 4xx errors. Status 415 always matches; 400/413/422 match only
  when the body contains multipart or content-type proxy fingerprints.

- **Browser auth storage mismatch** (`src/lib/utils/ipc.ts`): `getAuthUser()`
  now calls `authStorage()` (the same token-aware storage selector used by
  `setAuthUser()`) instead of a fixed localStorage-first fallback. Fixes stale
  cross-user reads when the remember-me state changes between sessions.

- **Admin storage exemption** (`src-tauri/src/webserver.rs`): verified already
  correct - `resolve_username()` returns `None` for all admin accounts, and
  storage limit/expiry checks already skip `None`. No code change required.

- **export_logs secrets** (`src-tauri/src/commands/api.rs`): verified already
  correct - `build_diagnostic_log` uses `cfgd()` (yes/no only) for all secret
  fields and excludes private tokens. No code change required.

- **Sync filesystem calls in async handlers** (`src-tauri/src/webserver.rs`):
  Changed `std::fs::read` to `tokio::fs::read(...).await` for the three
  gallery-image load paths and `std::fs::write` to `tokio::fs::write(...).await`
  for `save_image_file`.

- **Dropped Results** (`src-tauri/src/commands/api.rs`,
  `src-tauri/src/commands/prompt_assistant.rs`): replaced `let _ = ...` with
  `if let Err(e) = ... { log::warn!(...) }` for webhook dispatch (three call
  sites), attention backend uninstall, attention backend rollback, and
  `save_config` after prompt assistant model setup.

- **Dockerfile PyTorch index mismatch** (`Dockerfile`): changed
  `--index-url .../cu126` to `--index-url .../cu128` to match the `cu128`
  default used by `setup.rs`.

- **Path traversal in custom tagger model registration**
  (`src-tauri/src/commands/interrogator.rs`,
  `src-tauri/src/webserver.rs`): both desktop and server handlers now call
  `canonicalize()` before any I/O. The server handler additionally restricts the
  canonical path to the interrogator-managed models root to prevent remote
  callers from registering arbitrary filesystem paths.

- **Python silent except clauses** (`comfyui-nodes/nodes_sdxl_flux2vae.py`,
  `comfyui-nodes/nodes_sdxl_flux2vae_combined.py`,
  `comfyui-nodes/minimax_director/minimax_media.py`): replaced bare
  `except Exception: pass` with `except Exception as e: log.warning(...)` so
  failures surface in ComfyUI logs instead of being silently discarded.

Purpose: convert Gemini Code Assist and Copilot PR review comments into a practical action list for MooshieUI's actual deployment model: hosted over WAN for known, trusted users and moderators, not anonymous public users.

## How To Use This File

- `Fix soon`: real bugs or security/correctness issues that still matter under the current trust model.
- `Keep intentionally`: reviewer concern is understandable, but the current behavior is an intentional operator feature.
- `Low priority`: real issue, but not urgent.
- `Ignore / stale`: review comment is no longer relevant or was based on incomplete PR context.

## Fix Soon

### Auth and credentials

- Replace SHA-256 password hashing with Argon2id or bcrypt.
  - Current code still uses unsalted SHA-256 in `src-tauri/src/auth.rs`.
  - This is a real issue even for trusted WAN users.

- Remove checked-in default admin passwords. [Fixed 2026-09-04]
  - `.env.example` now ships with an empty password and a comment warning.
  - `k8s/secret.yaml` now uses a `REPLACE_ME` placeholder.
  - `server_main.rs` now refuses to create an admin account when the password equals `changeme`.

- Add session expiry / rotation.
  - Session tokens are persisted to `sessions.json` without TTL.
  - This is weaker than necessary for any remotely reachable auth system.

- Fix browser auth storage mismatch. [Fixed 2026-09-04]
  - `getAuthUser()` now uses `authStorage()` to match `setAuthUser()`.

### Real correctness bugs

- Fix RGBA conversion in `src-tauri/src/comfyui/mooshie_nodes.py`.
  - Current code assigns `rgba[:, :, :3] = img_np`.
  - If `img_np` is already RGBA, this can throw due to channel mismatch.
  - Safe fix: assign `img_np[:, :, :3]`.

- Fix prompt scheduling base text handling in `src/lib/utils/promptSchedule.ts`.
  - Scheduled text is still being added back into `baseText`.
  - That means scheduled prompt text can apply globally and within the scheduled range, which is incorrect.

- Fix storage/admin exemption logic in `src-tauri/src/webserver.rs`. [Verified correct 2026-09-04]
  - `resolve_username()` returns `None` for all admin accounts; storage checks skip `None`.
  - No code change was needed.

- Add timeout or bounded wait to output image finalization in `src/App.svelte`.
  - `Promise.allSettled(fetches)` still has no timeout.
  - A stuck image fetch can leave a generation hanging.

- Fix update banner timing in `src/lib/components/updater/UpdateNotification.svelte`.
  - Browser update check still runs one-shot on mount.
  - If auth/role resolution happens later, admin/mod users can miss the banner.

### Docker / deployment correctness

- Fix the Docker PyTorch version and wheel index. [Partially fixed 2026-09-04]
  - `Dockerfile` index changed from `cu126` to `cu128` to match `setup.rs`.
  - `TORCH_VERSION` was already updated to `2.11.0` in a prior pass.

## Fix Soon If Touching The Area

### Async runtime blocking

- Replace major synchronous filesystem work inside async handlers in `src-tauri/src/webserver.rs`.
  - [Partially fixed 2026-09-04] Hot-path gallery image reads and the save_image_file write converted to tokio::fs.
  - Gallery listing, metadata reading, storage info, and expiry cleanup remain synchronous for now.

### Export and logging safety

- Keep `export_logs`, but redact secrets before writing. [Verified correct 2026-09-04]
  - `build_diagnostic_log` already uses `cfgd()` (yes/no only) for all secret fields. No change needed.

### Accessibility and UI polish

- Add `aria-modal`, `aria-labelledby`, and `aria-describedby` to the storage modal in `src/lib/components/settings/SettingsPage.svelte`.
  - This is a real accessibility issue, just not urgent.

- Fix `wrap-break-word` in `PromptTextarea.svelte`.
  - It does not look like a valid Tailwind class.
  - Replace with `break-words`.

## Keep Intentionally

These were flagged by reviewers, but they align with the product's actual operating model.

### Moderator/operator capabilities

- Keep `install_pip_package` available to trusted moderators.
  - Face Fix depends on remote installation of `ultralytics`.
  - This is an intentional operator capability, not an accidental privilege escalation.
  - If desired later, narrow it to an allowlist rather than removing it entirely.

- Keep `download_model` available for remote operators.
  - Remote model installation is core product behavior.
  - Reviewer concern only makes sense under a hostile multi-tenant assumption.

- Keep `update_config` available to trusted remote operators if that is the intended role model.
  - This is configuration delegation, not necessarily a vulnerability.
  - Only revisit if moderators are meant to be lower-trust than they are today.

### Maybe keep, but harden

- `export_logs`
  - Keep feature.
  - Redact secrets.

- SSE token in query string for EventSource
  - Not ideal.
  - For trusted HTTPS deployments, this is lower severity than reviewers implied.
  - Improve later if convenient, but not top priority.

## Low Priority

### i18n cleanup

- Translate gallery expiry/storage strings in all non-English locale files.
- Add `gallery.toast.right_click_copy` to all locales.
  - This is real but not urgent.

### Auth / code cleanup items

- Log errors from `save_sessions()` instead of ignoring them silently.
- Consider implementing `Default` for `AuthState` only if it becomes useful.
- Simplify `map_or` usages if touched.
- Improve `flush_last_online()` so it does not keep stale activity forever.

### Misc frontend polish

- Fix lightbox null-guard around `selectedImage` in `gallery.svelte.ts`.
- Consider optimizing any repeated reactive filtering for large galleries.

## Ignore / Stale

### No longer relevant or already addressed

- Missing `connect_websocket_headless`
  - Stale review context. The function exists now.

- Face Fix supply-chain complaint about unpinned ultralytics
  - Fixed in v1.4.28. `mooshie-nodes/requirements.txt` pins `ultralytics==8.4.75`
    (the version verified against the bundled ComfyUI torch).

- Preview/output prompt isolation concerns from older reviews
  - Mostly addressed in current `App.svelte` with prompt filtering.

- v0.4.9 release-note mismatch complaints
  - Historical PR-scope noise. Repo now contains the referenced assets.

## Suggested Tomorrow Order

1. Password hashing
2. Remove default credentials from example/deployment files
3. Fix RGBA channel bug
4. Fix Docker PyTorch version/index
5. Fix admin storage/expiry exemption logic
6. Add timeout around output-image fetch wait
7. Start converting biggest blocking `std::fs` paths to `tokio::fs` or `spawn_blocking`
8. Redact secrets from exported logs
9. Fix browser auth storage mismatch
10. Clean up i18n and accessibility leftovers

## Notes On Reviewer Quality

- Gemini was mostly directionally correct on real bugs.
- The main pattern of overreach was assuming anonymous/public-host threat models instead of trusted remote operators.
- The most trustworthy review themes were:
  - password handling
  - default credentials
  - sync I/O in async handlers
  - Docker/runtime correctness
  - concrete code bugs like the RGBA assignment