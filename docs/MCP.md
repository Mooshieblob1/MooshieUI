# MCP server

MooshieUI can run a [Model Context Protocol](https://modelcontextprotocol.io) server so AI assistants (Claude Code, Claude Desktop, Cursor, LM Studio, chat bots and others) can generate, edit and upscale images (local models and NovelAI) and make music, use your saved characters, styles, chunks and wildcards, check the queue, search the gallery and your Eagle library, and download full-size images.

It is **off by default**. Turn it on in **Settings > AI assistants (MCP)**.

## How it works

- Listens on `http://127.0.0.1:3210/mcp` (the port is configurable). It never listens on the network, even with LAN mode on. Cloud apps reach it only through a tunnel you turn on (see [Cloud apps](#cloud-apps-claude-custom-connectors-chatgpt)).
- Runs in every mode: desktop window, browser mode and the headless server. In Docker, `127.0.0.1` is the container itself, so it is only reachable from inside the container.
- Every request needs `Authorization: Bearer <token>`, or the token in the path for apps that take only a URL: `POST /mcp/<token>`. The token is generated when the server is first enabled and can be replaced from Settings. Requests with a non-local `Host` or `Origin` header are refused (except your own public address, below), so a web page can't reach it.
- Transport: Streamable HTTP with plain JSON responses (protocol revisions 2025-06-18, 2025-03-26 and 2024-11-05). No server-initiated stream, so `GET /mcp` answers 405.
- Settings are kept in `mcp.json` in the app data folder, not in `config.json`.

## Generation setups

The frontend works out a model's family, loaders, text encoder and tuned defaults before every generation, and that can't be rebuilt from a filename on the backend. So the MCP server reuses what already works: each time the owner runs a generation, MooshieUI saves that request as the model's **setup** (in `mcp_setups.json`, without the prompt and without any input image). `generate_image` starts from a setup and changes only what the assistant asks for.

Three kinds of run make a setup, one per model each:

- **txt2img on a local model.** Used by `generate_image`.
- **txt2img on NovelAI**, keyed by the NovelAI model id (`nai-diffusion-4-5-full`, `nai-diffusion-5-full`, ...). It keeps the quality tags, UC preset, sampler, effort and the local post-process, and drops the character prompts, vibes and precise references (they belong to one image, and a vibe costs Anlas to encode).
- **Image Edit on a local model.** A Krea 2 setup keeps its edit recipe (Identity Edit, General Edit or Style Reference) and its fidelity settings, and uses the first two images.

A model is usable over MCP once it has been used for one such run in MooshieUI. `generate_image` with `source_image` (img2img) and `upscale_image` reuse the txt2img setup; `edit_image` needs an Image Edit one. NovelAI img2img, inpainting and upscale, local img2img and inpainting, and video runs don't create setups. Prompts are kept out of a setup, including the Moody Moss face and area detail prompts.

Images generated over MCP are saved to the owner's gallery (with normal metadata, `mooshie_source: mcp`) and show up in an open MooshieUI window right away.

### NovelAI costs nothing over MCP

An assistant can't spend Anlas. A NovelAI request over MCP must fit the Opus free window (one image, at most 1 megapixel such as 832x1216 or 1024x1024, at most 28 steps), the key in Settings must be on Opus (checked every 10 minutes), the face pass is held to free crops, and setups never carry vibes. Anything else is refused with a message saying why. NovelAI runs one request at a time per account, so MCP jobs wait their turn behind the Generate page's. The key used is the one active in Settings.

## Prompts: wildcards, characters, styles and chunks

`generate_image` prompts work like the Generate page's:

- `__name__` picks a line from a wildcard file in the wildcards folder (`.txt`, `.yaml`, `.json`, folders as `__folder/name__`, `*` globs), and `{a|b|c}` picks one choice (`{2$$a|b|c}`, weights `{3::a|b}` and nesting work too). Both are seeded by the job's seed, so a fixed seed repeats its picks. Wildcards set to "in order" in the editor pick at random over MCP, since their counters live in the window. Names that match no file are left in the prompt and listed in the result as `unknown_wildcards`.
- `characters` adds saved characters by name, picking the card saved for the model's family (else one written the same way, tags or sentences). On NovelAI V4 and later each becomes its own character prompt with its UC in that box; elsewhere its prompt goes at the end of the prompt and its UC into the negative prompt.
- `styles` appends a saved artist style's weighted artist tags and adds its UC.
- `chunks` adds saved prompt chunks: `prepend`, `append` or `random` (one random line). The default is the mode the chunk is active with in MooshieUI, else `append`. Their UC is added too.
- `use_active_styles: true` also adds the styles and chunks that are active in MooshieUI right now.

`list_library` lists all four, and `get_character` returns a character's full card, memory files and portrait. The library is read from the owner's synced data and can't be changed over MCP.

## Working from an existing image

Tools that take an image accept a gallery filename or the picture itself as base64 or a data URL (for one the assistant got elsewhere, such as a chat). Every input is decoded and re-encoded as a plain PNG before ComfyUI or NovelAI sees it, up to 8192 px a side and 25 MB.

- `generate_image` with `source_image` runs img2img (`denoise`, default 0.6). The size follows the source's aspect ratio at the setup's pixel count unless `width`/`height` are given. On NovelAI this is NovelAI img2img, still inside the free window.
- `generate_image` with `reference_images` (one or two) turns on the model's style reference: the Anima Reference Adapter, Flux Redux or IP-Adapter, with the setup's reference settings. When the owner's Anima setup uses SnapRef, it takes up to 8 images of the same character instead, at strength 1. Local models only.
- `edit_image` edits the first image with an instruction on an Image Edit model; more images are references.
- On a Ming-Image Layer setup, `edit_image` splits the image into transparent layers instead, one result per layer; the instruction names the count ("Decompose this image into 5 layers", default 4).
- `upscale_image` runs Refine: upscales by `scale` (1 to 4) and refines with the local model that made the image (or the one given), guided by the image's own prompt.

## Download links

Results that name an image carry a `url` that downloads the full-size file from the MooshieUI machine: `generate_image`, `edit_image`, `upscale_image`, `get_job`, `search_gallery`, `get_image`, and Eagle items in `search_eagle` and `get_eagle_image`. A chat bot can post the link or fetch the file. JPEG XL files are sent as PNG; other formats as they are, metadata included.

- Over the tunnel or your own address the link uses that public address; for apps on this machine it is `http://127.0.0.1:<port>`.
- A link is signed for that one file and expires after `link_hours` (default 24, at most 168). The signature is derived from the MCP token without revealing it, and **New token** in Settings voids every link handed out before.
- Anyone holding a link can download that file until it expires.

## Boards and Eagle

`list_boards` lists the gallery boards, and `search_gallery` filters by `board`, `favourites_only` and `min_rating`; results say each image's board, favourite and rating. Boards are read from the owner's synced data and can't be changed over MCP.

With [Eagle](https://eagle.cool) open on the MooshieUI machine, `search_eagle` searches its library by keyword, tags and folder (`list_folders: true` lists folders), `get_eagle_image` shows an item, and `send_to_eagle` adds gallery images the same way the gallery's Eagle export does (to the folder set in Settings, or `folder`).

## Music

`generate_music` uses the same YuE2 backend as the Music page. It needs no setup: the backend builds the YuE2 graph itself from the style, lyrics, length and seed, with the Music page's defaults (full planning, 32 steps, 120 seconds). `list_models` with `category: music` says whether YuE2 is installed and lists its checkpoints.

Finished songs are copied out of ComfyUI into the `music` folder in the app data folder, as `mcp_<date>_<time>_<seed>.flac` plus the ABC score YuE2 planned (`.abc`). The result gives the file paths; `include_audio: true` also embeds the FLAC (up to 16 MB) for clients that play MCP audio. Songs made over MCP don't appear in the Music page library, which is kept in the browser.

## Comic pages

`create_comic` does what the Comic page does. Pick a `layout` (the Comic page's templates, such as `one_over_two` or `grid_2x3`) or describe `rows`, then give one `panels` entry per panel in reading order. Describe the `cast` once with appearance tags and say which of them are in each panel; the page `style` and `setting` and the cast's looks are added to every panel, and a panel can replace the setting. Saved characters, styles and chunks work as in `generate_image`.

Each panel is an ordinary generation on the owner's setup for the model, sized to its box (NovelAI panels stay inside the free window), so each panel lands in the gallery. The page is then assembled at 2048 px on the long edge, with gutters and a black border round every panel, and saved to the gallery too. `right_to_left` reads rows right to left, as in manga.

On NovelAI each panel's `dialogue` is lettered in speech bubbles: the app adds `speech bubble` and the text tag for its language, and a `Text:` block last, as the Enhance V5 rules do. Other models draw no dialogue. `mode: one_image` (NovelAI V4 or later, up to six panels) draws the whole page as one image, each panel a character prompt placed at its centre.

A page takes several minutes, so with `wait: false`, or after the tunnel's 75 second limit, poll `get_job` with the `comic-...` job id. `redo_comic_panels` remakes some panels of a finished page, with new prompts or dialogue if given, keeps the rest and builds a new page. Comics made over MCP don't appear on the Comic page, which is kept in the browser.

## Tools

| Tool | What it does |
|------|--------------|
| `get_status` | ComfyUI status and GPUs, queue length, the last five image, comic and music jobs, and the saved setups. |
| `list_models` | Installed checkpoints, diffusion models (with `has_setup`), LoRAs, upscale models, or YuE2 music checkpoints. |
| `generate_image` | txt2img from a setup, local or NovelAI. Optional: `negative_prompt`, `model`, `width`, `height`, `steps`, `cfg`, `seed`, `batch_size` (1 to 8), `sampler`, `scheduler`, `loras`, `characters`, `styles`, `chunks`, `use_active_styles`, `source_image` + `denoise` (img2img or local outpainting with `outpaint_top`, `outpaint_bottom`, `outpaint_left`, `outpaint_right`, `outpaint_feather`), `reference_images`, `transparent_background` (Ming-Image Design). Waits up to 10 minutes and returns JPEG previews, or returns a job id with `wait: false`. |
| `generate_music` | A YuE2 song from `style`, with optional `lyrics` (omit for instrumental), `duration_seconds` (1 to 360), `checkpoint`, `planning` (`full`, `melody`, `off`), `score` (ABC to follow), `steps` and `seed`. Waits up to 10 minutes, or returns a job id with `wait: false`. |
| `create_comic` | A comic page: `layout` or `rows`, `panels` (prompt, `dialogue`, `cast`, `characters`, `setting`), `cast`, `style`, `setting`, `page_shape`, `right_to_left`, `gutter`, `mode` (`panels` or `one_image`) and the `generate_image` settings. Generates every panel, assembles the page and returns its preview and links. |
| `redo_comic_panels` | Remakes `panels` (or `changes` with new prompts or dialogue) of a finished comic and assembles a new page. |
| `get_job` | Status and results of an image, comic or music job (gallery filenames and previews, or song file paths), optionally waiting for it. |
| `cancel_generation` | Cancels one MCP job, or all of them. Never touches generations started in the window. |
| `search_gallery` | Full-text search over prompt, negative prompt and model, or the newest images, filtered by `board`, `favourites_only`, `min_rating`. Older images are indexed on demand, a few hundred per search. |
| `get_image` | A preview of a gallery image (up to 2048 px), its generation settings, board, rating and download link. |
| `edit_image` | Image Edit from an Image Edit setup: `images` (first is edited, the rest are references) and an instruction `prompt`. |
| `upscale_image` | Upscale and refine a gallery or base64 `image` by `scale` with a local model. |
| `list_boards` | Gallery boards with image counts. |
| `search_eagle` | Search the open Eagle library by `query`, `tags`, `folder`; or list its folders. |
| `get_eagle_image` | A preview of an Eagle item with its tags, notes and download link. |
| `send_to_eagle` | Add gallery images to Eagle. |
| `list_library` | Saved characters, artist styles, prompt chunks or wildcard files, by `kind`, with an optional name `filter`. |
| `get_character` | One saved character: its prompt and UC per model family, memory files and portrait. |

Sizes are snapped to multiples of 16 between 256 and 4096 px (64 on NovelAI).

## Cloud apps (Claude custom connectors, ChatGPT)

Claude's custom connectors (Claude Desktop, claude.ai, the mobile apps) and ChatGPT connect from their own servers, not from your computer, so `127.0.0.1` is unreachable for them. They need a public https address. Settings > AI assistants (MCP) offers two:

- **Cloudflare tunnel.** Turn it on and MooshieUI downloads `cloudflared` (a pinned release, checked against its SHA-256 before every start), runs a [quick tunnel](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/) and shows a connector URL like `https://<words>.trycloudflare.com/mcp/<token>`. No Cloudflare account is needed. The address changes every time the tunnel starts (each MooshieUI launch), so the connector has to be updated after a restart. Quick tunnels are meant for testing, have no uptime guarantee and allow 200 requests in flight. Settings only shows the URL once `cloudflared` has connected to Cloudflare. It needs outbound port 7844 (UDP for QUIC, TCP for the HTTP/2 fallback); a VPN such as Mullvad, or a firewall, that blocks it leaves the address answering with Cloudflare Error 1033, and Settings then says the connection looks blocked. Turn the VPN off or exclude `cloudflared.exe` (in `<data folder>/bin/cloudflared-<version>/`) with its split tunneling.
- **Your own address.** Run your own tunnel or reverse proxy (a Cloudflare named tunnel, Tailscale Funnel, an ngrok static domain, Caddy...), point it at `http://127.0.0.1:3210`, and enter its https address in Settings. Its host is then allowed through the `Host`/`Origin` checks and Settings shows a connector URL that never changes. A path prefix (`https://example.com/ai`) works too.

In Claude: **Settings > Connectors > Add custom connector**, paste the connector URL, no OAuth. Anyone with that URL can use MooshieUI from anywhere (generate, read the gallery), so keep it private and press **New token** if it leaks.

Cloudflare drops a request that gets no response within 100 seconds, so over a tunnel or proxy `generate_image`, `generate_music` and `get_job` wait at most 75 seconds, then return the job id; the assistant polls `get_job` for the rest.

Results that point at files on the MooshieUI machine (gallery filenames, `generate_music`'s FLAC path) still work from a cloud app: images come back as previews plus a download link through the tunnel, and `include_audio: true` returns the song itself.

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

**Apps that take only a URL**: use `http://127.0.0.1:3210/mcp/YOUR_TOKEN` (Settings shows it as "URL only").

**Claude Desktop without a tunnel**: its local config file only starts servers as commands, so bridge it with `mcp-remote` (needs Node.js). With a tunnel, add it as a custom connector instead (above).

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
- `setups.rs`: what a setup keeps (`snapshot`), what an assistant may change (`Overrides`, `apply`), the NovelAI free-window check, and the saved metadata map.
- `jobs.rs`: the job store, and image jobs: submits to the GPU manager (or to `novelai::run`, after the Opus check), follows `comfyui:*` events on the broadcast channel, saves outputs to the gallery, emits `mcp:image_saved`.
- `expand.rs`: `__wildcard__` and `{a|b}` expansion, a port of `utils/wildcards.ts` and `utils/randomPrompt.ts`. Keep them in step.
- `library.rs`: saved characters, styles and chunks from the owner's prefs, and how they go into a request (mirrors `toParams` and `savedCharacters.insert`).
- `music.rs`: music jobs. Maps the tool arguments to `MusicParams` (`to_params`), then submits, polls and downloads through `commands::music` (`submit`, `status`, `audio`), the same functions the Music page calls.
- `comic.rs`: comic pages. A port of the Comic page's layouts, reading order, panel sizes, prompt assembly and lettering (`utils/comicLayout.ts`) and its page assembly (`utils/comicRunner.ts`). Keep them in step. Each panel runs as an image job.
- `inputs.rs`: images an assistant passes in (gallery file or base64), uploaded to ComfyUI or sized for NovelAI.
- `links.rs`: signed, expiring download links and the `GET /files/...` route that serves them.
- `tools.rs`: tool schemas and handlers, including gallery boards (read from prefs) and the Eagle tools (`eagle.rs`).
- `tunnel.rs`: the optional Cloudflare quick tunnel. Pinned `cloudflared` per platform (`package`); bump `VERSION` and every hash together. It runs with `--http-host-header 127.0.0.1:<port>`, so tunnelled requests pass the local `Host` check. The state turns `running` only on a `Registered tunnel connection` line and drops back when the last connection goes (`connection_event`); the address alone does not mean the tunnel works.

When a feature changes, check whether the MCP surface needs to follow. See "MCP surface" in `CLAUDE.md`.
