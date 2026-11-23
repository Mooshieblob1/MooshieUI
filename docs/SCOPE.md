# MooshieUI Project Scope

This charter defines what MooshieUI is and is not. Use it to judge whether a
feature or change belongs in the project before writing code. When in doubt,
open an issue with the feature template and ask.

## What MooshieUI is

- A desktop (Tauri) and browser front-end for image, video, music and
  experimental image-to-3D generation through local or remote ComfyUI, with
  optional NovelAI image generation, and optional cloud voice and video generation (for example ElevenLabs and
  Seedance), each using the user's own API key and billed by that provider.
- A polished UI over generation workflows: prompt building, generation
  settings, gallery management, model hub, and related in-app tooling.
- Video generation on the same footing as images: shot planning through the
  timeline director, playback, frame interpolation, and clip export. The
  timeline is a shot list that feeds a ComfyUI graph, not a node editor.
- A lean built-in image editor for touching up generated images without
  losing their generation metadata.
- Music composition through ComfyUI, with style and lyric writing, a local song
  library, playlists, playback, manual lyric timing and audio export.
- Self-hosting with LAN accounts, roles, per-user galleries and configured GPU
  workers. Regular users and moderators manage their own NovelAI and cloud
  provider credentials.
- Windows, Linux and Apple Silicon macOS releases (Mac builds are
  user-tested), with native desktop and embedded browser-server modes sharing
  one codebase. See [Mac support status](MACOS.md).
- An opt-in, local, token-protected MCP server so AI assistants can drive the
  owner's existing generation setups, queue and gallery. See [MCP.md](MCP.md).
- Localized (i18n across all supported locales) and accessible (a11y).

## What MooshieUI is not

- Not a ComfyUI replacement or a general node-graph editor. It orchestrates
  ComfyUI; it does not reimplement it.
- Not an operated hosted SaaS service. Users run their own desktop or server;
  the existing multi-user self-hosting features are part of that scope.
- Not a model training or dataset-management tool.
- Not a painting or artwork suite. The built-in image editor covers photo-style
  touch-ups (crop, transform, colour adjustments and similar) so an edited
  image keeps its generation metadata; brush engines, vector tools, PSD and
  print workflows are out.
- Not a video editor or non-linear editing suite. Clips are planned before
  generation and exported afterwards; MooshieUI does not cut, composite, or
  grade footage. Joining a generated voice track to a generated clip at
  generation time is muxing, not editing.
- Never a way around a provider's safety filters. A provider's moderation
  block is shown to the user as final, not retried with a reworded prompt.
- Not a plugin marketplace or arbitrary third-party extension host.

## How scope is enforced

- Features are discussed in an issue (feature template) before implementation.
- Every PR states how it fits this charter (PR template).
- The maintainer judges scope against this document. Out-of-scope changes are
  declined regardless of code quality.
