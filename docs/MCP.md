# MCP server

MooshieUI can run a [Model Context Protocol](https://modelcontextprotocol.io) server so AI assistants (Claude Code, Claude Desktop, Cursor, LM Studio and others) can generate images and music, check the queue and search the gallery.

It is **off by default**. Turn it on in **Settings > AI assistants (MCP)**.

## How it works

- Listens on `http://127.0.0.1:3210/mcp` (the port is configurable). It never listens on the network, even with LAN mode on.
- Runs in every mode: desktop window, browser mode and the headless server. In Docker, `127.0.0.1` is the container itself, so it is only reachable from inside the container.
- Every request needs `Authorization: Bearer <token>`. The token is generated when the server is first enabled and can be replaced from Settings. Requests with a non-local `Host` or `Origin` header are refused, so a web page can't reach it.
- Transport: Streamable HTTP with plain JSON responses (protocol revisions 2025-06-18, 2025-03-26 and 2024-11-05). No server-initiated stream, so `GET /mcp` answers 405.
- Settings are kept in `mcp.json` in the app data folder, not in `config.json`.

## Generation setups

The frontend works out a model's family, loaders, text encoder and tuned defaults before every generation, and that can't be rebuilt from a filename on the backend. So the MCP server reuses what already works: each time the owner runs a plain txt2img generation, MooshieUI saves that request as the model's **setup** (in `mcp_setups.json`, without the prompt and without any input image). `generate_image` starts from a setup and changes only what the assistant asks for.

A model is usable over MCP once it has been used for one txt2img generation in MooshieUI. NovelAI, img2img, inpainting, Image Edit and video runs don't create setups.

Images generated over MCP are saved to the owner's gallery (with normal metadata, `mooshie_source: mcp`) and show up in an open MooshieUI window right away.

## Music

`generate_music` uses the same YuE2 backend as the Music page. It needs no setup: the backend builds the YuE2 graph itself from the style, lyrics, length and seed, with the Music page's defaults (full planning, 32 steps, 120 seconds). `list_models` with `category: music` says whether YuE2 is installed and lists its checkpoints.

Finished songs are copied out of ComfyUI into the `music` folder in the app data folder, as `mcp_<date>_<time>_<seed>.flac` plus the ABC score YuE2 planned (`.abc`). The result gives the file paths; `include_audio: true` also embeds the FLAC (up to 16 MB) for clients that play MCP audio. Songs made over MCP don't appear in the Music page library, which is kept in the browser.

## Tools

| Tool | What it does |
|------|--------------|
| `get_status` | ComfyUI status and GPUs, queue length, the last five image and music jobs, and the saved setups. |
| `list_models` | Installed checkpoints, diffusion models (with `has_setup`), LoRAs, or YuE2 music checkpoints. |
| `generate_image` | txt2img from a setup. Optional: `negative_prompt`, `model`, `width`, `height`, `steps`, `cfg`, `seed`, `batch_size` (1 to 8), `sampler`, `scheduler`, `loras`. Waits up to 10 minutes and returns JPEG previews, or returns a job id with `wait: false`. |
| `generate_music` | A YuE2 song from `style`, with optional `lyrics` (omit for instrumental), `duration_seconds` (1 to 360), `checkpoint`, `planning` (`full`, `melody`, `off`), `score` (ABC to follow), `steps` and `seed`. Waits up to 10 minutes, or returns a job id with `wait: false`. |
| `get_job` | Status and results of an image or music job (gallery filenames and previews, or song file paths), optionally waiting for it. |
| `cancel_generation` | Cancels one MCP job, or all of them. Never touches generations started in the window. |
| `search_gallery` | Full-text search over prompt, negative prompt and model, or the newest images. Older images are indexed on demand, a few hundred per search. |
| `get_image` | A preview of a gallery image (up to 2048 px) and its generation settings. |

Sizes are snapped to multiples of 16 between 256 and 4096 px.

## Connecting

Copy the command or JSON from Settings. For reference:

**Claude Code**

```bash
claude mcp add --transport http mooshieui http://127.0.0.1:3210/mcp --header "Authorization: Bearer YOUR_TOKEN"
```

**Apps that take an HTTP server in JSON** (Cursor, LM Studio, VS Code, ...)

```json
{
  "mcpServers": {
    "mooshieui": {
      "type": "http",
      "url": "http://127.0.0.1:3210/mcp",
      "headers": { "Authorization": "Bearer YOUR_TOKEN" }
    }
  }
}
```

**Claude Desktop** only starts local servers as commands, so bridge it with `mcp-remote` (needs Node.js):

```json
{
  "mcpServers": {
    "mooshieui": {
      "command": "npx",
      "args": ["-y", "mcp-remote", "http://127.0.0.1:3210/mcp", "--header", "Authorization:${MOOSHIE_AUTH}"],
      "env": { "MOOSHIE_AUTH": "Bearer YOUR_TOKEN" }
    }
  }
}
```

## For contributors

Code lives in `src-tauri/src/mcp/`:

- `mod.rs`: settings, listener lifecycle (`sync`), auth, JSON-RPC dispatch.
- `setups.rs`: what a setup keeps (`snapshot`), what an assistant may change (`Overrides`, `apply`), and the saved metadata map.
- `jobs.rs`: the job store, and image jobs: submits to the GPU manager, follows `comfyui:*` events on the broadcast channel, saves outputs to the gallery, emits `mcp:image_saved`.
- `music.rs`: music jobs. Maps the tool arguments to `MusicParams` (`to_params`), then submits, polls and downloads through `commands::music` (`submit`, `status`, `audio`), the same functions the Music page calls.
- `tools.rs`: tool schemas and handlers.

When a feature changes, check whether the MCP surface needs to follow. See "MCP surface" in `CLAUDE.md`.
