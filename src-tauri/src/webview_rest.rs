//! Lets the desktop WebView rest while nobody is using the app.
//!
//! The frontend pauses its own animations when idle (`src/lib/stores/idle.svelte.ts`);
//! this is the native half. WebView2 on Windows keeps rendering and holding its
//! caches while the window is minimised unless the host tells it otherwise:
//!
//! - minimising hides the WebView (`IsVisible = false`), which stops rendering
//!   and lets Chromium throttle timers, as Microsoft recommends for minimised
//!   apps; restoring shows it again;
//! - a resting UI (no input for a while, or hidden) lowers the WebView's memory
//!   usage target, so WebView2 trims caches and working set until someone comes
//!   back.
//!
//! WebKit (macOS, Linux) already throttles hidden windows on its own, so both
//! are no-ops there.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::error::AppError;

/// Last minimised state applied, so repeated resize events don't re-apply it.
static MINIMIZED: AtomicBool = AtomicBool::new(false);

/// Show or hide the main WebView as its window is minimised or restored.
pub fn sync_minimized(window: &WebviewWindow) {
    let minimized = window.is_minimized().unwrap_or(false);
    if MINIMIZED.swap(minimized, Ordering::AcqRel) == minimized {
        return;
    }
    log::debug!("[webview-rest] window minimised: {minimized}");
    set_visible(window, !minimized);
}

/// The frontend reports whether the UI is resting (idle or hidden, and nothing
/// generating); a resting UI asks WebView2 to keep its memory use low.
#[tauri::command]
pub async fn set_ui_resting(app: AppHandle, resting: bool) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("main") {
        set_low_memory(&window, resting);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_visible(window: &WebviewWindow, visible: bool) {
    let _ = window.with_webview(move |webview| {
        // SAFETY: the controller belongs to this live WebView and the call runs
        // on the thread that owns it.
        if let Err(e) = unsafe { webview.controller().SetIsVisible(visible) } {
            log::debug!("[webview-rest] SetIsVisible({visible}) failed: {e}");
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn set_visible(_window: &WebviewWindow, _visible: bool) {}

#[cfg(target_os = "windows")]
fn set_low_memory(window: &WebviewWindow, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows_core::Interface;

    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    let _ = window.with_webview(move |webview| {
        // SAFETY: as in `set_visible`. Runtimes older than ICoreWebView2_19
        // (WebView2 1.0.2210) fail the cast and keep their default level.
        let result = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_19>())
                .and_then(|core| core.SetMemoryUsageTargetLevel(level))
        };
        if let Err(e) = result {
            log::debug!("[webview-rest] SetMemoryUsageTargetLevel(low: {low}) failed: {e}");
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn set_low_memory(_window: &WebviewWindow, _low: bool) {}
