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
pub fn update(app: &AppHandle, recording: bool) {
    let app = app.clone();
    let result = app.clone().run_on_main_thread(move || {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return;
        };
        match build_menu(&app, recording) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => tracing::warn!("tray menu rebuild failed: {e}"),
        }
        let icon = if recording {
            recording_icon(&base_icon(&app))
        } else {
            base_icon(&app)
        };
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some(if recording { "Ogma — recording" } else { "Ogma" }));
    });
    if let Err(e) = result {
        tracing::warn!("tray update failed: {e}");
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
    let (w, h) = (base.width() as i32, base.height() as i32);
    let mut rgba = base.rgba().to_vec();
    let r = (w.min(h) * 3 / 10).max(2);
    let (cx, cy) = (w - r - 1, h - r - 1);
    for y in (cy - r).max(0)..(cy + r + 1).min(h) {
        for x in (cx - r).max(0)..(cx + r + 1).min(w) {
            let (dx, dy) = (x - cx, y - cy);
            if dx * dx + dy * dy <= r * r {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[0xE5, 0x3E, 0x3E, 0xFF]);
            }
        }
    }
    Image::new_owned(rgba, w as u32, h as u32)
}
