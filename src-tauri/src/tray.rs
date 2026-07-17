//! Tray icon and its menu.
//!
//! Interaction model differs by platform: on Linux (`StatusNotifierItem` /
//! `AppIndicator`) the tray delivers no click events, only a menu, so the menu
//! is the primary surface everywhere and macOS additionally gets richer
//! behaviour later.
//!
//! The icon itself comes from `meter-render`: usage state → RGBA. On macOS
//! the monochrome variant is marked as a template image so the system
//! recolours it to match the menu bar appearance; Linux trays get the
//! coloured variant. Live updates driven by the scheduler land with issue #4.

use meter_core::UsageStatus;
use meter_render::{IconCache, IconState, IconStyle, RenderedIcon, Scale};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

const TRAY_ID: &str = "main";

/// macOS menu-bar icons should be templates so they adapt to light/dark
/// appearance; Linux trays have no template concept, so colour carries state.
const MONO: bool = cfg!(target_os = "macos");

pub fn init<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Rusted Claude Meter", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        });

    // Empty gauge until the first snapshot arrives (issue #4 wires live
    // updates). A render failure falls back to the bundled app icon rather
    // than aborting startup.
    let mut cache = IconCache::new();
    let empty = IconState {
        style: IconStyle::Battery,
        percent: 0,
        status: UsageStatus::Safe,
        at_risk: false,
        mono: MONO,
        scale: Scale::X2,
    };
    match cache.get_or_render(empty) {
        Ok(icon) => {
            tray = tray
                .icon(tray_image(&icon))
                .icon_as_template(icon.is_template);
        }
        Err(error) => {
            eprintln!("tray icon render failed, using default icon: {error}");
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
        }
    }

    tray.build(app)?;
    Ok(())
}

/// Wrap rendered RGBA bytes in a tray image.
fn tray_image(icon: &RenderedIcon) -> Image<'static> {
    Image::new_owned(icon.rgba.clone(), icon.width, icon.height)
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
