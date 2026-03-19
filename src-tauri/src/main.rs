#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// On Wayland sessions, the AppImage's linuxdeploy GTK hook forces GDK_BACKEND=x11,
/// causing a white screen. Additionally, the bundled libwayland-client.so may be
/// incompatible with the host compositor. This function detects the problem and
/// re-execs the process with corrected environment variables (LD_PRELOAD must be set
/// before the dynamic linker runs, so a re-exec is required).
#[cfg(target_os = "linux")]
fn fix_wayland_appimage_env() {
    use std::path::Path;

    // Only apply when running on Wayland
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        return;
    }

    // Use a sentinel to avoid infinite re-exec loop
    if std::env::var("_MOOSHIEUI_WAYLAND_FIXED").is_ok() {
        return;
    }

    let mut needs_reexec = false;

    // Remove forced X11 backend (set by linuxdeploy-plugin-gtk)
    if std::env::var("GDK_BACKEND").ok().as_deref() == Some("x11") {
        std::env::remove_var("GDK_BACKEND");
        // GDK_BACKEND removal takes effect without re-exec (read by GTK init)
    }

    // Preload system libwayland-client if we're in an AppImage and LD_PRELOAD isn't set
    if std::env::var("APPIMAGE").is_ok() && std::env::var("LD_PRELOAD").is_err() {
        let candidates = [
            "/usr/lib/libwayland-client.so",
            "/usr/lib/x86_64-linux-gnu/libwayland-client.so",
            "/usr/lib64/libwayland-client.so",
        ];
        for path in &candidates {
            if Path::new(path).exists() {
                std::env::set_var("LD_PRELOAD", path);
                needs_reexec = true;
                break;
            }
        }
    }

    // Disable DMABuf renderer to avoid WebKit/Wayland GPU buffer issues
    if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    if needs_reexec {
        use std::os::unix::process::CommandExt;

        // Mark that we've applied the fix, then re-exec
        std::env::set_var("_MOOSHIEUI_WAYLAND_FIXED", "1");
        let exe = std::env::current_exe().expect("failed to get current exe");
        let args: Vec<String> = std::env::args().skip(1).collect();
        let err = std::process::Command::new(&exe).args(&args).exec();
        // exec() only returns on error — if we get here, just continue
        eprintln!("Wayland fix: re-exec failed: {}", err);
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    fix_wayland_appimage_env();

    comfyui_desktop_lib::run();
}
