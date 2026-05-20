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
    tauri_build::build();
}
