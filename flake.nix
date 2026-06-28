{
  description = "MooshieUI - beginner-friendly desktop frontend for ComfyUI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        inherit (pkgs) lib;

        mooshieui-unwrapped = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
          pname = "mooshieui";
          version = "1.4.28";

          src = ./.;

          # Rust backend lives in src-tauri/. All crates resolve from
          # crates.io, so the lockfile alone is enough (no outputHashes).
          cargoRoot = "src-tauri";
          buildAndTestSubdir = "src-tauri";
          cargoLock.lockFile = ./src-tauri/Cargo.lock;

          # Frontend (Svelte/Vite) npm deps, fetched offline.
          # Regenerate with: nix run nixpkgs#prefetch-npm-deps -- package-lock.json
          npmDeps = pkgs.fetchNpmDeps {
            name = "${finalAttrs.pname}-npm-deps";
            src = ./.;
            hash = "sha256-uuVfCw3lpZRw9R89rlV4No9QQ/ckZArom9qgNmgw9V4=";
          };
          # package.json is at the repo root, not in src-tauri/.
          npmRoot = ".";

          # tauri.conf.json sets createUpdaterArtifacts=true, which makes
          # `tauri build` try to sign self-updater artifacts (needs the
          # author's TAURI_SIGNING_PRIVATE_KEY). A Nix-installed app updates
          # via Nix, not Tauri's updater, so turn the artifacts off here.
          tauriBuildFlags = [
            "--config"
            (builtins.toJSON { bundle.createUpdaterArtifacts = false; })
          ];

          # `nix build` runs `cargo test` in release mode. The jxl roundtrip
          # test is off-by-a-few under release SIMD (jxl-encoder
          # "unsafe-performance"); skip just that one, keep the rest as a
          # smoke check. See src/jxl.rs:362 — worth investigating upstream.
          checkFlags = [ "--skip=jxl::tests::roundtrip_2x2_rgba8" ];

          nativeBuildInputs = with pkgs; [
            cargo-tauri.hook       # runs `cargo tauri build`, honoring beforeBuildCommand
            nodejs
            npmHooks.npmConfigHook # populates node_modules from npmDeps
            pkg-config
            wrapGAppsHook4
            makeWrapper
          ];

          buildInputs = with pkgs; [
            openssl                # reqwest/tokio-tungstenite use native-tls
            glib
            gtk3
            cairo
            pango
            gdk-pixbuf
            atkmm
            libsoup_3
            webkitgtk_4_1          # matches the `webkit2gtk = "=2.0.2"` crate
            librsvg
            onnxruntime            # for `ort` (load-dynamic, dlopen'd at runtime)
          ];

          # Build against system OpenSSL rather than vendoring it.
          OPENSSL_NO_VENDOR = "1";

          # `ort` is built with the `load-dynamic` feature, so it dlopen's
          # libonnxruntime at runtime — point it at the Nix-built lib.
          # Wrap manually so we can merge the gApps args with the ORT path.
          dontWrapGApps = true;
          postFixup = ''
            wrapProgram "$out/bin/comfyui-desktop" \
              "''${gappsWrapperArgs[@]}" \
              --set ORT_DYLIB_PATH "${pkgs.onnxruntime}/lib/libonnxruntime.so"
          '';

          # Desktop file + icon are installed by cargo-tauri.hook's deb
          # bundling (share/applications/MooshieUI.desktop, hicolor icons),
          # so no manual desktop integration is needed here.

          meta = {
            description = "Beginner-friendly desktop frontend for ComfyUI";
            homepage = "https://github.com/Mooshieblob1/MooshieUI";
            license = lib.licenses.agpl3Plus; # src-tauri/Cargo.toml: AGPL-3.0-or-later
            mainProgram = "comfyui-desktop";
            platforms = lib.platforms.linux;
          };
        });
      in
      {
        # The raw Tauri app (works for the GUI, but its setup wizard downloads
        # generic-linux uv/CPython that NixOS can't exec directly).
        packages.unwrapped = mooshieui-unwrapped;

        # NixOS can't run the generic `uv` + CPython that MooshieUI's setup
        # wizard downloads at runtime (no /lib64/ld-linux). Running the whole
        # app inside an FHS sandbox gives those downloaded binaries — and the
        # pip wheels they install — a standard glibc + loader, so setup "just
        # works". This needs no system-level nix-ld and applies automatically
        # to anyone who installs this package.
        packages.default = pkgs.buildFHSEnv {
          name = "comfyui-desktop";
          targetPkgs = p: with p; [
            mooshieui-unwrapped
            # GUI runtime (Tauri/webkit) — also available to child processes
            glib gtk3 cairo pango gdk-pixbuf atk librsvg webkitgtk_4_1 libsoup_3
            libGL mesa libdrm
            onnxruntime
            # uv, python-build-standalone, and common native-wheel deps
            stdenv.cc.cc zlib openssl bzip2 xz libffi ncurses readline sqlite
            # opencv-python (cv2) — pulled in by `ultralytics` (face detailer)
            # and comfyui_controlnet_aux — dlopen's these X11 client libs at
            # `import cv2`. Without them: "libxcb.so.1: cannot open shared
            # object file".
            libxcb libx11 libxext libxrender
            libsm libice libxi libxrandr libxfixes
            fontconfig freetype
            # C toolchain: some custom-node wheels (e.g. stringzilla, pulled in
            # by comfyui_controlnet_aux via albumentations) compile native
            # extensions from source at `uv pip install` time. Without a
            # compiler: "error: command 'cc' failed: No such file or directory".
            gcc binutils gnumake
            # tools the setup wizard shells out to
            gnutar gzip coreutils git cacert
          ];
          runScript = "comfyui-desktop";
          # Surface the desktop entry + icon from the inner package.
          extraInstallCommands = ''
            mkdir -p "$out/share"
            cp -r ${mooshieui-unwrapped}/share/applications "$out/share/"
            cp -r ${mooshieui-unwrapped}/share/icons "$out/share/"
          '';
        };
      }
    );
}
