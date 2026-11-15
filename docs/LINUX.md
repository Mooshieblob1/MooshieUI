# Installing on Linux

MooshieUI ships for x86_64 Linux as an AppImage, a `.deb` and an `.rpm` on every
[release](https://github.com/Mooshieblob1/MooshieUI/releases). You can install
it through your package manager instead, so it updates with the rest of your
system.

| Distro | Install | Update |
| --- | --- | --- |
| Arch, Manjaro, EndeavourOS | [coming soon](#arch-aur): `yay -S mooshieui-bin` | `yay -Syu` |
| Debian, Ubuntu, Mint, Pop!_OS | [add the apt repo](#debian-and-ubuntu), then `sudo apt install mooshieui` | `sudo apt update && sudo apt upgrade` |
| Fedora, RHEL clones | [add the rpm repo](#fedora), then `sudo dnf install mooshieui` | `sudo dnf upgrade --refresh` |
| Anything else | download the `.AppImage`, `chmod +x` it and run it | in-app updater |

The apt and dnf repos carry every release after v2.3.46. Until the AUR
package is published, Arch users can use the AppImage.

When a package manager owns the install, MooshieUI's update banner shows the
command above instead of downloading the update itself.

All routes install only the app. On first launch the setup wizard downloads
Python, PyTorch and ComfyUI into your data folder (`~/.local/share/com.mooshieui.desktop`
unless you pick another), the same as on Windows. Removing the package leaves
that folder and your models in place. Setup needs `git`; the packages pull it
in.

## Arch (AUR)

Coming soon: new AUR accounts are paused for now, so the package isn't
published yet. Once it is, [`mooshieui-bin`](https://aur.archlinux.org/packages/mooshieui-bin)
repackages the release `.deb`, is updated automatically on every release, and
installs with an AUR helper:

```bash
yay -S mooshieui-bin    # or: paru -S mooshieui-bin
```

## Debian and Ubuntu

Ubuntu 22.04 or newer, Debian 12 or newer (the app needs WebKitGTK 4.1). Add the
signing key and the repo once:

```bash
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://mooshieui.com/mooshieui.asc | sudo tee /etc/apt/keyrings/mooshieui.asc > /dev/null
sudo tee /etc/apt/sources.list.d/mooshieui.sources > /dev/null <<'SOURCES'
Types: deb
URIs: https://mooshieui.com/apt
Suites: stable
Components: main
Architectures: amd64
Signed-By: /etc/apt/keyrings/mooshieui.asc
SOURCES
sudo apt update
sudo apt install mooshieui
```

## Fedora

```bash
sudo curl -fsSLo /etc/yum.repos.d/mooshieui.repo https://mooshieui.com/rpm/mooshieui.repo
sudo dnf install mooshieui
```

dnf asks you to accept the MooshieUI signing key the first time.

## Verifying the key

The repo metadata is signed with the key at
[mooshieui.com/mooshieui.asc](https://mooshieui.com/mooshieui.asc), which is the
same file as [`packaging/linux/mooshieui.asc`](../packaging/linux/mooshieui.asc)
in this repository. Check its fingerprint with
`gpg --show-keys /etc/apt/keyrings/mooshieui.asc`; it should be
`1F82 986C 6CC1 D890 5E48  FE0D FF29 34B6 0954 B5D7`
(MooshieUI Packages <blob@mooshieblob.com>). The packages themselves are
the unmodified release files; the signed metadata carries their SHA-256.

## Uninstalling

Remove the package (`sudo apt remove mooshie-ui`, `sudo dnf remove mooshie-ui`,
`yay -R mooshieui-bin`), then delete the repo file
(`/etc/apt/sources.list.d/mooshieui.sources` or `/etc/yum.repos.d/mooshieui.repo`)
and key. Delete `~/.local/share/com.mooshieui.desktop` too if you want the
runtime and models gone.

## Flatpak

There is no Flatpak yet. The first-run install of Python, PyTorch and ComfyUI
does not fit the Flatpak sandbox well (GPU access in particular), so the
AppImage is the distro-independent option for now.
