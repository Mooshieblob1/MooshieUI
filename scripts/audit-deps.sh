#!/usr/bin/env bash
# Dependency advisory audit shared by the release build (release.yml), the
# Dependency Audit workflow (PRs, main and a daily run) and the release skills,
# so a new advisory shows up before a tag instead of failing the release build.
#
# npm: a high or critical advisory in a shipped (non-dev) dependency fails.
# Dev-only advisories are left to Dependabot.
#
# Rust: vulnerabilities fail. Informational advisories (unmaintained, unsound,
# yanked) only warn: most come from the gtk-rs/webkit stack Tauri pins on Linux
# (glib, gtk, zbus/zvariant) and have no release to move to. Fix a
# vulnerability with `cargo update -p <crate>` (check whether the crate that
# pulls it in has a newer release first, `cargo tree -i <crate>@<version>`);
# only one with no fixed release reachable from our dependency tree goes on the
# ignore list below, with the reason, and comes off again once a fix lands.
#
# Needs npm and cargo-audit (`cargo install cargo-audit --locked`).
set -euo pipefail

cd "$(dirname "$0")/.."

echo "== npm audit (shipped dependencies, high and above)"
npm audit --omit=dev --audit-level=high

echo "== cargo audit"
ignored=(
  # --ignore RUSTSEC-YYYY-NNNN  # crate: why there is no fix yet
)
cd src-tauri
cargo audit "${ignored[@]}"
