# Report Proxy Runbook

Self-hosted service on the NUC (192.168.4.80) that turns in-app error reports into
GitHub issues on Mooshieblob1/MooshieUI. The GitHub credential lives only here.

## One-time setup

### 1. GitHub token
Create a fine-grained PAT: GitHub Settings -> Developer settings -> Fine-grained tokens.
- Repository access: only Mooshieblob1/MooshieUI.
- Permissions: Issues -> Read and write. Nothing else.

### 2. Cloudflare Tunnel (done in the dashboard)
- Zero Trust -> Networks -> Tunnels -> Create a tunnel -> Cloudflared.
- Name it (e.g. mooshie-report), save. Copy the token from the install command
  (the long string after `service install`). Do NOT install cloudflared by hand; the
  compose runs it as a container.
- Public Hostname (a.k.a. Published Application route) tab -> Add a public hostname:
  - Subdomain: report
  - Domain: mooshieblob.com
  - Type: HTTP
  - URL: localhost:8091

  Saving this auto-creates the proxied `report.mooshieblob.com` CNAME. Make sure the
  route is added to the same tunnel whose token is in `.env`.

### 3. Secrets on the NUC
The file /home/blob/report-proxy/.env (perms 600) holds:
    GITHUB_TOKEN=...
    CLOUDFLARE_TUNNEL_TOKEN=...

## Deploy

    cd /home/blob/report-proxy
    docker compose up -d --build

cloudflared shares the proxy's network namespace and forwards
report.mooshieblob.com to localhost:8091.

## Origin policy (CORS)

Reports come from two kinds of client, and only one has a fixed origin:

- The desktop app posts from its webview: `tauri://localhost` (macOS/Linux),
  `http://tauri.localhost` or `https://tauri.localhost` (Windows).
- Browser mode posts from wherever the user's own MooshieUI server is reachable:
  localhost, a LAN address, a tunnel or custom domain. No allowlist can name these.

So the proxy (src/origin.rs) does not restrict to a list, and instead:

- Accepts the Tauri origins and any plain `http(s)://host[:port]` origin at the
  CORS preflight. Every report must carry `X-Mooshie-App: 1`, which forces that
  preflight in a browser.
- Refuses `Origin: null` (sandboxed iframes, file: pages), extension and other
  schemes, and malformed values, both at the preflight and in the handler (403).
- Accepts requests with no Origin header (curl, scripts). CORS never limited
  those; the header gate, per-IP rate limit and budgets do.
- Charges reports from web origins (browser mode, or any website that makes its
  visitors' browsers post) against a separate share of the global write budget,
  `WEB_ORIGIN_WRITES_PER_MIN` (default 3 of the 8 per minute). A page abusing
  this can therefore create at most that many issues or comments a minute, and
  can never use up the budget the desktop app depends on.

Tighten further (for example only the Tauri origins) if browser-mode reports ever
become abuse: `allowed_by_cors` and `classify` in src/origin.rs are the one place
to change, and browser-mode users would then fall back to the prefilled GitHub
issue path.

## Smoke test
    curl -s -X POST https://report.mooshieblob.com/report \
      -H "X-Mooshie-App: 1" -H "Content-Type: application/json" \
      -d '{"errorCode":"generic","rawMessage":"smoke test","appVersion":"0","os":"x","arch":"x","mode":"desktop","timestamp":"2026-07-05T00:00:00Z"}'
Expect JSON: {"issueUrl":"https://github.com/Mooshieblob1/MooshieUI/issues/N"}.
Send the same payload again; expect the same issueUrl (deduped via a comment).

## Logs
    docker compose logs -f report-proxy
    docker compose logs -f cloudflared
