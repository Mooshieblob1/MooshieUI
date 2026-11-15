# Linux package repository

`scripts/linux-repo.sh` builds signed apt and rpm metadata for each stable
release (the `linux-repo` job in `.github/workflows/release.yml`). The files are
published as `apt-*` / `rpm-*` release assets, and mooshieui.com serves them:

| URL | Goes to |
| --- | --- |
| `https://mooshieui.com/apt/dists/stable/...` | `releases/latest/download/apt-*` |
| `https://mooshieui.com/apt/pool/<tag>/<file>.deb` | `releases/download/<tag>/<file>.deb` |
| `https://mooshieui.com/rpm/repodata/<file>` | `releases/latest/download/rpm-<file>` |
| `https://mooshieui.com/rpm/mooshieui.repo` | static file on the site |
| `https://mooshieui.com/mooshieui.asc` | static copy of `mooshieui.asc` here |

The redirects live in mooshieui-site `static/_redirects`. The rpm metadata's
`xml:base` points dnf straight at the release's `.rpm`.

## Signing key

`mooshieui.asc` in this folder is the public half of the repo signing key. The
release job only signs when the `REPO_GPG_PRIVATE_KEY` secret (and
`REPO_GPG_PASSPHRASE`, if the key has one) holds that exact key, and skips the
repo with a warning when either is missing. The site serves the same public key
at `https://mooshieui.com/mooshieui.asc`; change both together.

The installed app looks for `/etc/apt/sources.list.d/mooshieui.sources`,
`/etc/yum.repos.d/mooshieui.repo` and `/etc/zypp/repos.d/mooshieui.repo`
(`src-tauri/src/package_manager.rs`) to hand updates to the package manager, so
the install instructions must use those file names.
