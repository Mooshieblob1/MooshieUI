#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(feature = "desktop")]
    comfyui_desktop_lib::run();

    #[cfg(not(feature = "desktop"))]
    eprintln!(
        "This binary requires the 'desktop' feature. Use mooshieui-server for headless mode."
    );
}
