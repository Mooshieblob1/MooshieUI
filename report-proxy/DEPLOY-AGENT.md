# Deploy the MooshieUI report-proxy (instructions for the NUC agent)

You are an agent operating on the NUC (192.168.4.80, Arch Linux, user `blob`, CPU-only).
Your job: deploy the `report-proxy` service plus its `cloudflared` sidecar with Docker
Compose. Follow these steps in order. Do not skip verification steps. Never print secret
token values to the terminal or logs.

## What you are deploying

A small Rust + axum service (`report-proxy`) plus a `cloudflared` sidecar, run via Docker
Compose. It receives error reports from the MooshieUI app and files GitHub issues on
Mooshieblob1/MooshieUI, attaching the logs, system info, and the user's message. No host
port is published; only the Cloudflare Tunnel reaches it. CPU-only is fine.

## What the human provides (not you)

- A fine-grained GitHub PAT: repository access limited to Mooshieblob1/MooshieUI only,
  permission Issues -> Read and write, nothing else.
- A Cloudflare Tunnel token, created by the human in the Cloudflare dashboard
  (Zero Trust -> Networks -> Tunnels -> Create a tunnel -> Cloudflared), with a public
  hostname (Published Application route) `report.mooshieblob.com` routed to Type HTTP,
  URL `localhost:8091`, on the same tunnel whose token is in `.env`.

The public URL is `https://report.mooshieblob.com/report`.

If you do not have these, ask the human for them before continuing.

## Step 0: Verify prerequisites (do not assume)

    docker version && docker compose version
    docker network ls | grep blob_default

- `docker compose` must be the v2 plugin form (not `docker-compose`).
- `blob_default` network must exist. If it is missing, stop and report to the human.

## Step 1: Get the code onto the NUC

    cd /home/blob
    git clone https://github.com/Mooshieblob1/MooshieUI.git tmp-mooshie \
      || (cd /home/blob/tmp-mooshie && git pull)
    # If PR #428 is not yet merged to main, check out the branch instead:
    #   cd /home/blob/tmp-mooshie && git checkout report-proxy && git pull
    cp -r /home/blob/tmp-mooshie/report-proxy /home/blob/report-proxy
    cd /home/blob/report-proxy

You only need the `report-proxy/` subdirectory. The full clone is just a convenient way
to fetch it.

## Step 2: Create the secrets file

Do NOT echo the token values or commit `.env`.

    cp .env.example .env
    # Edit .env and fill in exactly these two lines (leave the commented overrides alone):
    #   GITHUB_TOKEN=<fine-grained PAT from the human>
    #   CLOUDFLARE_TUNNEL_TOKEN=<tunnel token from the human>
    chmod 600 .env

## Step 3: Sanity-check the compose file

Confirm `docker-compose.yml` has:
- service `report-proxy` joining the external `blob_default` network, with NO `ports:`
  mapping (only cloudflared reaches it).
- service `cloudflared` using `network_mode: "service:report-proxy"` and
  `command: tunnel --no-autoupdate run --token ${CLOUDFLARE_TUNNEL_TOKEN}`.
- `networks: blob_default: external: true`.

Do not change the file; just verify it matches before starting.

## Step 4: Build and start

    docker compose up -d --build
    docker compose ps

Both `report-proxy` and `report-cloudflared` should show `Up`.

## Step 5: Verify

Check the app started and cloudflared connected:

    docker compose logs report-proxy | tail -20     # expect: report-proxy listening on 0.0.0.0:8091
    docker compose logs cloudflared | tail -20      # expect: Registered tunnel connection

Internal health check. No host port is published, so hit it from inside the network with a
throwaway container (the runtime image has no curl/wget, so `docker compose exec` will not
work for this):

    docker run --rm --network blob_default curlimages/curl:latest \
      -s -o /dev/null -w "health: %{http_code}\n" http://report-proxy:8091/health
    # Expect: health: 200

Public end-to-end smoke test:

    curl -s -X POST https://report.mooshieblob.com/report \
      -H "X-Mooshie-App: 1" -H "Content-Type: application/json" \
      -d '{"errorCode":"generic","rawMessage":"smoke test","appVersion":"0","os":"x","arch":"x","mode":"desktop","timestamp":"2026-07-05T00:00:00Z"}'
    # Expect JSON: {"issueUrl":"https://github.com/Mooshieblob1/MooshieUI/issues/N"}
    # Send the SAME payload again; expect the SAME issueUrl (deduped via a comment, not a new issue).

If you created a test issue, tell the human its URL so they can close it.

## Step 6: Report back

Report to the human: both containers' status, the health check result, the cloudflared
connection state, and the smoke-test issue URL (or the exact error if it failed).

Response code meanings if the smoke test fails:
- 403: the `X-Mooshie-App: 1` header is missing or wrong.
- 429: rate-limited (fixed window per IP); wait a minute and retry.
- 400: payload invalid or `errorCode` empty.
- 502: GitHub rejected the token; re-check the PAT scope (this repo only, Issues read+write).

## Gotchas

- No host port is published by design. Use the throwaway-curl-container method in Step 5,
  not `curl localhost:8091`.
- The runtime image is `debian:bookworm-slim` with no curl/wget inside, so do not try
  `docker compose exec` to curl the health endpoint.
- `cloudflared` shares the proxy's network namespace, so from its perspective it forwards
  to `localhost:8091`. That is expected and matches the dashboard public-hostname setting.
- Never echo the tokens. They live only in `/home/blob/report-proxy/.env` (chmod 600).

## Updating later

After code changes:

    cd /home/blob/tmp-mooshie && git pull
    cp -r /home/blob/tmp-mooshie/report-proxy/. /home/blob/report-proxy/
    cd /home/blob/report-proxy && docker compose up -d --build

See `RUNBOOK.md` for full detail.
