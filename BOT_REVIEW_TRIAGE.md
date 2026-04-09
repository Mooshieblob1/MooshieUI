# Bot Review Triage

Updated: 2026-04-10

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

- Remove checked-in default admin passwords.
  - `.env.example` still contains `MOOSHIEUI_ADMIN_PASS=changeme`.
  - `k8s/secret.yaml` still contains the base64 for `changeme`.
  - These are easy deployment footguns.

- Add session expiry / rotation.
  - Session tokens are persisted to `sessions.json` without TTL.
  - This is weaker than necessary for any remotely reachable auth system.

- Fix browser auth storage mismatch.
  - `setAuthUser()` and `getAuthUser()` still read/write different storage backends in some flows.
  - This can mix per-user client state when switching remember-me behavior.

### Real correctness bugs

- Fix RGBA conversion in `src-tauri/src/comfyui/mooshie_nodes.py`.
  - Current code assigns `rgba[:, :, :3] = img_np`.
  - If `img_np` is already RGBA, this can throw due to channel mismatch.
  - Safe fix: assign `img_np[:, :, :3]`.

- Fix prompt scheduling base text handling in `src/lib/utils/promptSchedule.ts`.
  - Scheduled text is still being added back into `baseText`.
  - That means scheduled prompt text can apply globally and within the scheduled range, which is incorrect.

- Fix storage/admin exemption logic in `src-tauri/src/webserver.rs`.
  - Authenticated admin accounts are still treated differently from localhost admin.
  - Storage limits and expiry still appear to apply to admin user directories.

- Add timeout or bounded wait to output image finalization in `src/App.svelte`.
  - `Promise.allSettled(fetches)` still has no timeout.
  - A stuck image fetch can leave a generation hanging.

- Fix update banner timing in `src/lib/components/updater/UpdateNotification.svelte`.
  - Browser update check still runs one-shot on mount.
  - If auth/role resolution happens later, admin/mod users can miss the banner.

### Docker / deployment correctness

- Fix the Docker PyTorch version and wheel index.
  - `Dockerfile` still uses `TORCH_VERSION=2.7.1`.
  - It still points to `cu126`.
  - Both are likely invalid and should be corrected before relying on that image.

## Fix Soon If Touching The Area

### Async runtime blocking

- Replace major synchronous filesystem work inside async handlers in `src-tauri/src/webserver.rs`.
  - This remains one of the most consistently valid reviewer themes.
  - Examples still include gallery listing, metadata reading, storage info, expiry cleanup, and path-based image operations.
  - These are not theoretical; they can hurt responsiveness under normal use.

### Export and logging safety

- Keep `export_logs`, but redact secrets before writing.
  - The feature is useful for trusted remote operators.
  - Dumping config with secrets intact is still sloppy and should be cleaned up.

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
  - Already fixed. Current code pins `ultralytics==8.4.34`.

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