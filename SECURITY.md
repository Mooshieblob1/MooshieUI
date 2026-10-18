# Security Policy

## Reporting a vulnerability

Please report security issues privately by email to **security@mooshieui.com**.
Do not open a public issue for anything that could put users at risk.

Include what you can of:

- the version (Settings > About, or the release tag) and platform
- which mode is affected: desktop app, browser mode on localhost, LAN mode, or
  the Docker / self-hosted server
- steps to reproduce or a proof of concept
- the impact you think it has

I'm a solo maintainer working on this in my spare time (GMT+8), so expect an
acknowledgement within a few days. Fixes ship in a normal release, and the
release notes credit the reporter unless you'd rather stay anonymous.

## Supported versions

Only the latest release gets security fixes, so please check the issue still
exists on the newest version before reporting.

## Scope

In scope: this repository, meaning the desktop app, the embedded browser/LAN
server, the headless server build and Docker image (`deploy/`), the custom
ComfyUI nodes in `comfyui-nodes/`, and the error report proxy
(`report-proxy/`).

Out of scope: ComfyUI itself, third-party custom nodes, model files, and the
external services MooshieUI can talk to (NovelAI, ElevenLabs, CivitAI, Hugging
Face and others). Report those to their own maintainers.
