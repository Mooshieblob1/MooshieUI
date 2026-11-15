#!/usr/bin/env bash
# Build the signed apt and rpm repository metadata for one release.
#
#   scripts/linux-repo.sh <tag> <deb> <rpm> <out-dir>
#
# Writes flat release assets to <out-dir>: apt-InRelease, apt-Release,
# apt-Release.gpg, apt-Packages, apt-Packages.gz, rpm-repomd.xml,
# rpm-repomd.xml.asc and the other rpm-* repodata files. mooshieui.com serves
# them through redirects to releases/latest/download (mooshieui-site
# static/_redirects), and points package downloads at this tag's assets:
#   https://mooshieui.com/apt  -> deb  [signed-by mooshieui.asc]
#   https://mooshieui.com/rpm  -> rpm  (repo_gpgcheck)
#
# Signs with the key MOOSHIEUI_REPO_KEY (a fingerprint) from the default
# GNUPGHOME; MOOSHIEUI_REPO_PASSPHRASE_FILE, when set, unlocks it. The packages
# themselves are not re-signed, so their bytes stay identical to the release
# assets; the signed metadata carries their SHA-256.
#
# Needs apt-ftparchive (apt-utils), createrepo_c and gpg.
set -euo pipefail

tag="${1:?usage: linux-repo.sh <tag> <deb> <rpm> <out-dir>}"
deb="${2:?}"
rpm="${3:?}"
out="${4:?}"
key="${MOOSHIEUI_REPO_KEY:?set MOOSHIEUI_REPO_KEY to the signing key fingerprint}"
repo="${MOOSHIEUI_GITHUB_REPO:-Mooshieblob1/MooshieUI}"

gpg_sign=(gpg --batch --yes --local-user "$key")
if [ -n "${MOOSHIEUI_REPO_PASSPHRASE_FILE:-}" ]; then
  gpg_sign+=(--pinentry-mode loopback --passphrase-file "$MOOSHIEUI_REPO_PASSPHRASE_FILE")
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

# --- apt ---------------------------------------------------------------------
# Filename: pool/<tag>/<deb>; the site redirects /apt/pool/* to
# releases/download/*, so each index points at its own release's .deb.
apt="$work/apt"
dist="$apt/dists/stable"
mkdir -p "$apt/pool/$tag" "$dist/main/binary-amd64"
cp "$deb" "$apt/pool/$tag/"
(cd "$apt" && apt-ftparchive packages pool > "$dist/main/binary-amd64/Packages")
gzip -9kn "$dist/main/binary-amd64/Packages"
(
  cd "$dist"
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=MooshieUI \
    -o APT::FTPArchive::Release::Label=MooshieUI \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    -o "APT::FTPArchive::Release::Description=MooshieUI ${tag}" \
    release . > "$work/Release"
)
mv "$work/Release" "$dist/Release"
"${gpg_sign[@]}" --clearsign --digest-algo SHA512 -o "$dist/InRelease" "$dist/Release"
"${gpg_sign[@]}" --detach-sign --armor --digest-algo SHA512 -o "$dist/Release.gpg" "$dist/Release"
for file in InRelease Release Release.gpg; do
  cp "$dist/$file" "$out/apt-$file"
done
cp "$dist/main/binary-amd64/Packages" "$out/apt-Packages"
cp "$dist/main/binary-amd64/Packages.gz" "$out/apt-Packages.gz"

# --- rpm ---------------------------------------------------------------------
# xml:base sends dnf straight to this release's .rpm asset.
rpmdir="$work/rpm"
mkdir -p "$rpmdir"
cp "$rpm" "$rpmdir/"
createrepo_c --quiet --no-database --simple-md-filenames --general-compress-type=gz \
  --baseurl "https://github.com/${repo}/releases/download/${tag}/" "$rpmdir"
"${gpg_sign[@]}" --detach-sign --armor --digest-algo SHA512 \
  -o "$rpmdir/repodata/repomd.xml.asc" "$rpmdir/repodata/repomd.xml"
for file in "$rpmdir"/repodata/*; do
  cp "$file" "$out/rpm-$(basename "$file")"
done

ls -1 "$out"
