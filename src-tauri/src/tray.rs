//! System-tray quick-start (PLAN.md backlog #2): start/stop recording without
//! opening the main window.
//!
//! The tray offers a Start/Stop Recording toggle, Open Ogma, and Quit; while
//! recording, the icon gains a red dot and the tooltip says so. Left-click
//! opens the window. Closing the main window hides to the tray instead of
//! exiting (see `on_window_event` in lib.rs) — Quit lives here.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "ogma-tray";

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, false)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(base_icon(app))
        .tooltip("Ogma")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-toggle" => crate::toggle_recording(app.clone()),
            "tray-open" => show_main_window(app),
            "tray-quit" => crate::quit(app.clone()),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Reflect the recording state in the tray (menu label, icon dot, tooltip).
/// Tray/menu mutation must happen on the main thread on some platforms.
///
/// With the window hidden, the tray is the only sign the mic is live — so a
/// failure to indicate `recording == true` must never be silent: every path
/// logs, and the fallback is showing the main window (whose record view
/// reflects the live recording).
pub fn update(app: &AppHandle, recording: bool) {
    let handle = app.clone();
    let result = app.run_on_main_thread(move || {
        let mut indicated = false;
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            match build_menu(&handle, recording) {
                Ok(menu) => {
                    if let Err(e) = tray.set_menu(Some(menu)) {
                        tracing::warn!("tray menu update failed: {e}");
                    }
                }
                Err(e) => tracing::warn!("tray menu rebuild failed: {e}"),
            }
            let icon = if recording {
                recording_icon(&base_icon(&handle))
            } else {
                base_icon(&handle)
            };
            let icon_ok = match tray.set_icon(Some(icon)) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("tray icon update failed: {e}");
                    false
                }
            };
            let tooltip = if recording { "Ogma — recording" } else { "Ogma" };
            let tooltip_ok = match tray.set_tooltip(Some(tooltip)) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("tray tooltip update failed: {e}");
                    false
                }
            };
            indicated = icon_ok && tooltip_ok;
        } else {
            tracing::warn!("tray icon not found; recording state not reflected");
        }
        if recording && !indicated {
            show_main_window(&handle);
        }
    });
    if let Err(e) = result {
        tracing::warn!("tray update failed: {e}");
        if recording {
            show_main_window(app);
        }
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn build_menu(app: &AppHandle, recording: bool) -> tauri::Result<Menu<Wry>> {
    let toggle = MenuItem::with_id(
        app,
        "tray-toggle",
        if recording { "■ Stop Recording" } else { "● Start Recording" },
        true,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(app, "tray-open", "Open Ogma", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit Ogma", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&toggle, &sep, &open, &quit])
}

fn base_icon(app: &AppHandle) -> Image<'static> {
    match app.default_window_icon() {
        Some(icon) => Image::new_owned(icon.rgba().to_vec(), icon.width(), icon.height()),
        // Should not happen (icons are bundled); a transparent pixel keeps the
        // tray alive rather than panicking over cosmetics.
        None => Image::new_owned(vec![0, 0, 0, 0], 1, 1),
    }
}

/// The app icon with a red "recording" dot in the bottom-right corner, drawn
/// directly on the RGBA buffer to avoid an image-crate dependency.
fn recording_icon(base: &Image<'_>) -> Image<'static> {
    let mut rgba = base.rgba().to_vec();
    draw_dot(&mut rgba, base.width(), base.height());
    Image::new_owned(rgba, base.width(), base.height())
}

const DOT_RGBA: [u8; 4] = [0xE5, 0x3E, 0x3E, 0xFF];

/// Paint the red disc into the bottom-right corner of a `w`×`h` RGBA buffer.
/// Pure so the index arithmetic is unit-testable (a regression here would
/// panic on the main thread mid-recording-start).
fn draw_dot(rgba: &mut [u8], w: u32, h: u32) {
    let (w, h) = (w as i32, h as i32);
    let r = (w.min(h) * 3 / 10).max(2);
    let (cx, cy) = (w - r - 1, h - r - 1);
    for y in (cy - r).max(0)..(cy + r + 1).min(h) {
        for x in (cx - r).max(0)..(cx + r + 1).min(w) {
            let (dx, dy) = (x - cx, y - cy);
            if dx * dx + dy * dy <= r * r {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&DOT_RGBA);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{draw_dot, DOT_RGBA};

    fn buf(w: u32, h: u32) -> Vec<u8> {
        vec![0u8; (w * h * 4) as usize]
    }

    #[test]
    fn draw_dot_stays_in_bounds_on_degenerate_sizes() {
        // 1x1 is the transparent-pixel fallback icon; the rest probe the
        // clamp math around a radius larger than the buffer.
        for (w, h) in [(1, 1), (2, 2), (1, 3), (3, 1), (4, 4)] {
            let mut b = buf(w, h);
            draw_dot(&mut b, w, h); // must not panic
        }
    }

    #[test]
    fn draw_dot_paints_bottom_right_and_leaves_top_left() {
        let (w, h) = (32u32, 32u32);
        let mut b = buf(w, h);
        draw_dot(&mut b, w, h);
        // Dot center (as computed by draw_dot) is red.
        let r = (32i32 * 3 / 10).max(2);
        let (cx, cy) = (32 - r - 1, 32 - r - 1);
        let center = ((cy * 32 + cx) * 4) as usize;
        assert_eq!(&b[center..center + 4], &DOT_RGBA);
        // Rightmost pixel of the disc (cx + r, cy) is painted…
        let edge = ((cy * 32 + cx + r) * 4) as usize;
        assert_eq!(&b[edge..edge + 4], &DOT_RGBA);
        // …and the disc never spills past the buffer edge (cx + r ≤ 31).
        assert!(cx + r <= 31);
        // Top-left stays untouched.
        assert_eq!(&b[0..4], &[0, 0, 0, 0]);
    }
}
