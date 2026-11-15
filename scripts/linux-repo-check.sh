#!/usr/bin/env bash
# End-to-end check of scripts/linux-repo.sh against a published release, with
# a throwaway signing key: apt (on this Ubuntu host) and dnf (in a Fedora
# container) must both accept the signed metadata and resolve `mooshieui`'s
# package. Run by the Linux repo check workflow on PRs.
#
#   scripts/linux-repo-check.sh <tag>
set -euo pipefail

tag="${1:?usage: linux-repo-check.sh <tag>}"
version="${tag#v}"
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'kill "${server:-0}" 2>/dev/null || true; rm -rf "$work"' EXIT
base="https://github.com/Mooshieblob1/MooshieUI/releases/download/${tag}"
deb="MooshieUI_${version}_amd64.deb"
rpm="MooshieUI-${version}-1.x86_64.rpm"

mkdir -p "$work/release"
curl -fsSL --retry 3 -o "$work/release/$deb" "$base/$deb"
curl -fsSL --retry 3 -o "$work/release/$rpm" "$base/$rpm"

export GNUPGHOME="$work/gnupg"
install -d -m 700 "$GNUPGHOME"
gpg --batch --quiet --passphrase '' --quick-gen-key "MooshieUI repo check <check@invalid>" rsa3072 sign never
fpr=$(gpg --batch --with-colons --list-keys | awk -F: '/^fpr/{print $10; exit}')
gpg --batch --armor --export "$fpr" > "$work/key.asc"
MOOSHIEUI_REPO_KEY="$fpr" bash "$root/scripts/linux-repo.sh" "$tag" "$work/release/$deb" "$work/release/$rpm" "$work/assets"

# Lay the flat assets out the way mooshieui.com's redirects present them.
site="$work/site"
mkdir -p "$site/apt/dists/stable/main/binary-amd64" "$site/apt/pool/$tag" "$site/rpm/repodata"
for f in InRelease Release Release.gpg; do cp "$work/assets/apt-$f" "$site/apt/dists/stable/$f"; done
for f in Packages Packages.gz; do cp "$work/assets/apt-$f" "$site/apt/dists/stable/main/binary-amd64/$f"; done
cp "$work/release/$deb" "$site/apt/pool/$tag/"
for f in "$work"/assets/rpm-*; do cp "$f" "$site/rpm/repodata/$(basename "$f" | sed 's/^rpm-//')"; done
python3 -m http.server 8765 --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 &
server=$!
sleep 1

echo "--- apt"
cat > "$work/mooshieui.sources" <<SRC
Types: deb
URIs: http://127.0.0.1:8765/apt
Suites: stable
Components: main
Architectures: amd64
Signed-By: $work/key.asc
SRC
mkdir -p "$work/apt-lists/partial" "$work/apt-cache/archives/partial"
apt_opts=(-o "Dir::Etc::SourceList=$work/mooshieui.sources" -o Dir::Etc::SourceParts=/dev/null
  -o "Dir::State::Lists=$work/apt-lists" -o "Dir::Cache=$work/apt-cache" -o Debug::NoLocking=1
  -o APT::Sandbox::User=root)
sudo apt-get "${apt_opts[@]}" update
# Only this repo is configured, so check the candidate rather than a full
# install, whose dependencies come from the distro's own repos.
apt-cache "${apt_opts[@]}" policy mooshie-ui | grep "Candidate: ${version}"
(cd "$work" && sudo apt-get "${apt_opts[@]}" download mooshie-ui)
cmp "$work"/mooshie-ui_*_amd64.deb "$work/release/$deb"

echo "--- dnf"
cat > "$work/mooshieui.repo" <<REPO
[mooshieui]
name=MooshieUI
baseurl=http://127.0.0.1:8765/rpm
enabled=1
gpgcheck=0
repo_gpgcheck=1
gpgkey=file:///check/key.asc
REPO
docker run --rm --network host -v "$work:/check:ro" fedora:latest bash -euo pipefail -c '
  cp /check/mooshieui.repo /etc/yum.repos.d/
  dnf -y -q --disablerepo="*" --enablerepo=mooshieui makecache
  dnf -y --disablerepo="*" --enablerepo=mooshieui download --destdir /tmp mooshie-ui
  cmp /tmp/MooshieUI-*.rpm /check/release/'"$rpm"'
'
echo "apt and dnf both accepted the signed repo for $tag"
