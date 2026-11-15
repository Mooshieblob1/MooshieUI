#!/usr/bin/env bash
# Render the mooshieui-bin PKGBUILD for a released version, build it with
# makepkg in an Arch container to prove it works, and write PKGBUILD and
# .SRCINFO to <out-dir> for the AUR.
#
#   scripts/aur-package.sh <version> <out-dir>
#
# Needs curl, sha256sum and docker. Used by the publish-aur release job and the
# AUR package check on pull requests.
set -euo pipefail

version="${1:?usage: aur-package.sh <version> <out-dir>}"
out="${2:?usage: aur-package.sh <version> <out-dir>}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "AUR pkgver must look like 1.2.3, got '$version'" >&2
  exit 1
fi

# archlinux:base-devel, pinned. Only makepkg and bsdtar run in it, with no
# pacman sync, so an older image is fine.
image="archlinux@sha256:fa2d4ce3310ddd90a4208e5168be8ad367da5129af1245f38fd2d803bcbe3239"
root="$(cd "$(dirname "$0")/.." && pwd)"
deb="MooshieUI_${version}_amd64.deb"

mkdir -p "$out"
out="$(cd "$out" && pwd)"
curl -fsSL --retry 3 -o "$out/$deb" \
  "https://github.com/Mooshieblob1/MooshieUI/releases/download/v${version}/${deb}"
sha="$(sha256sum "$out/$deb" | cut -d' ' -f1)"
sed -e "s/@PKGVER@/${version}/" -e "s/@SHA256@/${sha}/" \
  "$root/packaging/aur/mooshieui-bin/PKGBUILD.in" > "$out/PKGBUILD"

# makepkg refuses to run as root, so build as a throwaway user. The .deb is
# already in place, so makepkg only checks its hash instead of downloading.
docker run --rm -e HOST_IDS="$(id -u):$(id -g)" -v "$out:/pkg" "$image" bash -euo pipefail -c '
  useradd -m builder
  chown -R builder /pkg
  su builder -c "cd /pkg && makepkg --nodeps --noconfirm --force && makepkg --printsrcinfo > .SRCINFO"
  pkg=$(ls /pkg/mooshieui-bin-*.pkg.tar.*)
  bsdtar -tf "$pkg" | grep -qx "usr/bin/comfyui-desktop"
  bsdtar -tf "$pkg" | grep -qx "usr/share/applications/MooshieUI.desktop"
  [ "$(bsdtar -xOf "$pkg" usr/share/mooshieui/package-manager)" = pacman ]
  chown -R "$HOST_IDS" /pkg
'
echo "Built $(ls "$out"/mooshieui-bin-*.pkg.tar.*) and wrote $out/PKGBUILD + .SRCINFO"
