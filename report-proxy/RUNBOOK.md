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

## Smoke test
    curl -s -X POST https://report.mooshieblob.com/report \
      -H "X-Mooshie-App: 1" -H "Content-Type: application/json" \
      -d '{"errorCode":"generic","rawMessage":"smoke test","appVersion":"0","os":"x","arch":"x","mode":"desktop","timestamp":"2026-07-05T00:00:00Z"}'
Expect JSON: {"issueUrl":"https://github.com/Mooshieblob1/MooshieUI/issues/N"}.
Send the same payload again; expect the same issueUrl (deduped via a comment).

## Logs
    docker compose logs -f report-proxy
    docker compose logs -f cloudflared
