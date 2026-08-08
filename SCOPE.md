# MooshieUI Project Scope

This charter defines what MooshieUI is and is not. Use it to judge whether a
feature or change belongs in the project before writing code. When in doubt,
open an issue with the feature template and ask.

## What MooshieUI is

- A desktop (Tauri) and browser front-end for running local image and video
  generation through ComfyUI, focused on a fast, friendly, single-user
  experience.
- A polished UI over generation workflows: prompt building, generation
  settings, gallery management, model hub, and related in-app tooling.
- Video generation on the same footing as images: shot planning through the
  timeline director, playback, frame interpolation, and clip export. The
  timeline is a shot list that feeds a ComfyUI graph, not a node editor.
- Cross-platform (Windows, macOS, Linux) and dual-mode (native desktop app and
  an embedded browser server), sharing one codebase.
- Localized (i18n across all supported locales) and accessible (a11y).

## What MooshieUI is not

- Not a ComfyUI replacement or a general node-graph editor. It orchestrates
  ComfyUI; it does not reimplement it.
- Not a multi-tenant or hosted SaaS product. It targets a single user's machine
  (or their own self-hosted browser instance).
- Not a model training or dataset-management tool.
- Not a general-purpose image editor beyond the specific in-app editing features
  already shipped.
- Not a video editor or non-linear editing suite. Clips are planned before
  generation and exported afterwards; MooshieUI does not cut, composite, or
  grade footage.
- Not a plugin marketplace or arbitrary third-party extension host.

## How scope is enforced

- Features are discussed in an issue (feature template) before implementation.
- Every PR states how it fits this charter (PR template).
- The maintainer judges scope against this document. Out-of-scope changes are
  declined regardless of code quality.
