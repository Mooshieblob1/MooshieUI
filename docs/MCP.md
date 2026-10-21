# MCP server

MooshieUI can run a [Model Context Protocol](https://modelcontextprotocol.io) server so AI assistants (Claude Code, Claude Desktop, Cursor, LM Studio and others) can generate images, check the queue and search the gallery.

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

## Tools

| Tool | What it does |
|------|--------------|
| `get_status` | ComfyUI status and GPUs, queue length, the last five MCP jobs, and the saved setups. |
| `list_models` | Installed checkpoints, diffusion models (with `has_setup`) or LoRAs. |
| `generate_image` | txt2img from a setup. Optional: `negative_prompt`, `model`, `width`, `height`, `steps`, `cfg`, `seed`, `batch_size` (1 to 8), `sampler`, `scheduler`, `loras`. Waits up to 10 minutes and returns JPEG previews, or returns a job id with `wait: false`. |
| `get_job` | Status, gallery filenames and previews of a job, optionally waiting for it. |
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
- `jobs.rs`: submits to the GPU manager, follows `comfyui:*` events on the broadcast channel, saves outputs to the gallery, emits `mcp:image_saved`.
- `tools.rs`: tool schemas and handlers.

When a feature changes, check whether the MCP surface needs to follow. See "MCP surface" in `CLAUDE.md`.
