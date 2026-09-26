fn main() {
    // rust-embed requires `../dist/` to exist at compile time. In CI or a
    // fresh checkout we may build before `npm run build` has produced the
    // frontend bundle, so create an empty placeholder if it's missing.
    let dist = std::path::Path::new("../dist");
    if !dist.exists() {
        let _ = std::fs::create_dir_all(dist);
    }
    println!("cargo:rerun-if-changed=../dist");

    #[cfg(feature = "desktop")]
    build_desktop();
}

#[cfg(feature = "desktop")]
fn build_desktop() {
    let mut attributes = tauri_build::Attributes::new();

    // tauri (muda/rfd) statically imports Common Controls v6-only entry points
    // such as `TaskDialogIndirect`, so every exe linking it needs a manifest
    // that selects comctl32 v6, or the loader binds v5.82 and the process dies
    // at startup with STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139). tauri-build
    // embeds that manifest as a resource via `rustc-link-arg-bins`, which never
    // reaches the lib unit-test harness (`cargo test --lib`). Cargo has no
    // link-arg scope for lib tests, so on MSVC have the linker embed the same
    // dependency for every linked target instead, and drop tauri-build's copy
    // so bins don't end up with two RT_MANIFEST resources.
    // /MANIFESTUAC:NO keeps the release exe's manifest identical to
    // tauri-build's default (no requestedExecutionLevel).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }

    if let Err(error) = tauri_build::try_build(attributes) {
        // Mirrors tauri_build::build(), which is try_build(Attributes::default()).
        println!("{error:#}");
        std::process::exit(1);
    }
}
