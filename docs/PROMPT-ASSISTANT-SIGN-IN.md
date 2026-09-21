# Prompt Assistant account sign-in (unreleased)

In **Settings > Prompt Assistant**, enable the external assistant and choose:

| Provider | Sign-in uses | Supported inputs |
| --- | --- | --- |
| ChatGPT (subscription) | Your ChatGPT account through the official Codex app server | Text and images |
| Gemini (Google sign-in) | Your Google account through Google's official Antigravity ACP companion | Text, images and audio style analysis |
| Anthropic / Claude | Your existing Anthropic API key | Existing text and image API support |

Choose **Sign in**. The app downloads a pinned, checksum-verified official
companion release into its own data directory, then opens the provider's login
flow. Gemini also uses the Node.js runtime prepared at app startup for a hook
that blocks agent tools. No npm,
administrator access, global installation or manual token copying is required.
Sign-in can be cancelled. An empty model field uses the companion's default;
**Load models** lists the models that companion makes available to your account.

Google ended Gemini CLI access for individual Free, AI Pro and AI Ultra accounts
on June 18, 2026. The Gemini option now uses Google's official Antigravity ACP
server. If you tried the older connection, sign in again: credentials are not
copied between clients. Google opens in your normal browser profile while
`GEMINI_HOME` keeps companion settings, credentials and sessions private to this app.
The official file-storage option also isolates credentials from the macOS keychain.
The first download is approximately 300–700 MB, depending on the platform;
starting the companion can take several seconds.

These options use account quotas, which may depend on your plan, region and
eligibility. They do not provide unlimited free use, turn a chat subscription
into a general API subscription, or generate the song audio themselves. YuE2
still generates music through ComfyUI. MooshieUI does not switch these routes to
API keys when a request fails or the quota is exhausted. Any extra usage enabled
directly with the provider remains subject to that account's settings.

Sign-in is currently desktop-only because the provider's browser callback runs
on the host computer. A browser/server installation can use an account already
configured on that same host and app-data directory. It uses the host operator's
account, not a separate subscription for each browser user. Provider settings
retain their existing moderator permission requirement in browser mode.

Each companion keeps its OAuth credentials in a private MooshieUI account
directory. The app does not read tokens or copy them into API-key settings. Your
separate Codex/Gemini installations and sign-ins are untouched. **Sign out**
removes this app's local account storage; it does not revoke other provider
sessions. ChatGPT's agent tools are disabled. Antigravity's official PreToolUse
hook rejects every tool call, and MooshieUI also refuses ACP file and permission
requests. Requests stop on cancellation, timeout or app shutdown; cleanup stops
the companion's owned child processes as well as its launcher.
Temporary request workspaces are removed. Gemini's local conversation cache is
also cleared after the companion exits, including when an audio analysis is
cancelled. ChatGPT requests use ephemeral threads. Login state remains until
sign-out; the providers' data policies still apply to submitted content.

ChatGPT's release manifest covers Windows x64/ARM64, macOS Intel/Apple Silicon
and Linux x64/ARM64. Google's current Antigravity release covers those platforms
except Intel Macs. Source and compile checks do not replace testing on each OS.
Google account sign-in, model selection, text and audio input, and a denied
file-tool attempt have been verified with the official Antigravity server
on Windows. These checks do not measure music-analysis quality. The account-free development smoke test initializes real tools
with empty isolated account directories and makes no inference request.

Provider references checked September 21, 2026:

- [Codex app-server integration and authentication](https://learn.chatgpt.com/docs/app-server)
- [Google's retirement of Gemini CLI access for individual accounts](https://github.com/google-gemini/gemini-cli/discussions/28017)
- [Official Antigravity ACP distribution](https://github.com/agentclientprotocol/registry/blob/main/antigravity-acp/agent.json)
- [Antigravity account sign-in for ACP clients](https://antigravity.google/docs/ide/extensions/zed)
- [Antigravity hooks](https://antigravity.google/docs/hooks)
- [Antigravity plans](https://antigravity.google/docs/plans)
- [Claude Code authentication restrictions and embedding conditions](https://code.claude.com/docs/en/legal-and-compliance)

Direct Claude.ai subscription OAuth for a third-party assistant is not the
supported route. Embedding unmodified Claude Code has separate conditions and
is outside this integration; Claude keeps its existing API-key behavior.
