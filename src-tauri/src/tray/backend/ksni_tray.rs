//! The Linux tray: a `StatusNotifierItem` we own directly, via `ksni`.
//!
//! Tauri's `tray-icon` goes through libappindicator on Linux, whose SNI object
//! deliberately omits `Activate` — so a left-click has no method for the panel
//! to invoke and no click can ever reach the app. Speaking the protocol
//! ourselves is what makes click-to-popover parity with macOS possible.
//!
//! What the desktops then do with `Activate` is theirs to decide, and they
//! disagree: KDE Plasma calls it on left-click, while GNOME's `AppIndicator`
//! extension binds it to *double*-click and keeps single-click for the menu.
//! Both are handled by the same `activate` below; the difference is policy we
//! do not control.
//!
//! ## Why the state lives behind an `Arc<Mutex<..>>`
//!
//! `ksni::Handle::update` is async, but [`super::TrayBackend`]'s pushes are
//! synchronous and run inside the scheduler's tokio task — `block_on` there
//! would panic ("cannot start a runtime from within a runtime"). So the new
//! value is written into shared state *synchronously*, and a no-op `update` is
//! spawned purely to make ksni re-read the properties and emit the change
//! signals. Writing the data first also makes the ordering safe: two spawned
//! updates may complete out of order, but both would then read the same,
//! latest state rather than racing a stale one onto the panel.

use std::sync::{Arc, Mutex};

use ksni::menu::{MenuItem as KsniMenuItem, StandardItem};
use ksni::{Handle, Icon, ToolTip, Tray, TrayMethods};
use meter_render::RenderedIcon;
use tauri::{AppHandle, Manager, Runtime};

use super::TrayBackend;
use super::icon_file::IconFiles;
use crate::scheduler::SchedulerHandle;
use crate::sync::lock;
use crate::tray::model::MenuModel;
use crate::tray::{show_main_window, window};

/// What the tray shows. Written by the backend, read by the [`Tray`] impl.
struct Shared {
    /// Themed icon name — the primary route, because GNOME squashes
    /// `IconPixmap` into a square (see [`super::icon_file`]).
    icon_name: String,
    icon_theme_path: String,
    /// The same image as raw pixels, for hosts that ignore theme paths. Hosts
    /// prefer `IconName` when it is non-empty, so this is a fallback only.
    icon: Option<Icon>,
    menu: MenuModel,
}

/// The `StatusNotifierItem` itself.
struct MeterTray<R: Runtime> {
    app: AppHandle<R>,
    shared: Arc<Mutex<Shared>>,
}

impl<R: Runtime> Tray for MeterTray<R> {
    fn id(&self) -> String {
        "rusted-claude-meter".to_owned()
    }

    fn title(&self) -> String {
        "Rusted Claude Meter".to_owned()
    }

    fn icon_name(&self) -> String {
        lock(&self.shared).icon_name.clone()
    }

    fn icon_theme_path(&self) -> String {
        lock(&self.shared).icon_theme_path.clone()
    }

    /// Only ever a fallback for when no icon file could be written — never
    /// alongside a name.
    ///
    /// Publishing both loses the correct rendering on GNOME. Its extension
    /// tries the name first, and for a wide image takes a branch that draws the
    /// image itself at its true aspect and then returns *no* gicon
    /// (`_loadCustomImage` → `return null`). `_createIcon` reads that null as
    /// "the name produced nothing" and falls through to `_createIconFromPixmap`,
    /// which forces a square — undoing the good rendering. libappindicator
    /// published only the name, which is why the icon looked right before.
    fn icon_pixmap(&self) -> Vec<Icon> {
        let shared = lock(&self.shared);
        if shared.icon_name.is_empty() {
            shared.icon.clone().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    /// A real hover tooltip, which libappindicator could not offer. The pace
    /// line is the same text the menu carries; showing it on hover costs
    /// nothing and is the one place Linux ends up slightly ahead of macOS.
    fn tool_tip(&self) -> ToolTip {
        let shared = lock(&self.shared);
        ToolTip {
            title: shared.menu.status_line.clone(),
            description: shared.menu.pace_line.clone().unwrap_or_default(),
            ..ToolTip::default()
        }
    }

    /// Left-click on KDE, double-click on GNOME: toggle the popover window —
    /// the macOS `NSPopover` behaviour, as close as the protocol allows.
    fn activate(&mut self, x: i32, y: i32) {
        window::toggle(&self.app, x, y);
    }

    fn menu(&self) -> Vec<KsniMenuItem<Self>> {
        let shared = lock(&self.shared);
        build_menu(&shared.menu)
    }
}

/// The same menu shape the macOS tray builds: status line, the pace line when
/// present, live usage lines (informational, disabled), then the actions.
fn build_menu<R: Runtime>(menu: &MenuModel) -> Vec<KsniMenuItem<MeterTray<R>>> {
    let mut items: Vec<KsniMenuItem<MeterTray<R>>> = vec![info_item(&menu.status_line)];
    if let Some(pace) = &menu.pace_line {
        items.push(info_item(pace));
    }
    if !menu.usage_lines.is_empty() {
        items.push(KsniMenuItem::Separator);
        items.extend(menu.usage_lines.iter().map(|line| info_item(line)));
    }
    items.push(KsniMenuItem::Separator);
    items.push(action_item(
        "Open Rusted Claude Meter",
        |tray: &mut MeterTray<R>| {
            show_main_window(&tray.app);
        },
    ));
    items.push(action_item("Settings…", |tray: &mut MeterTray<R>| {
        crate::settings_window::open(&tray.app);
    }));
    items.push(action_item("Refresh Now", |tray: &mut MeterTray<R>| {
        if let Some(scheduler) = tray.app.try_state::<SchedulerHandle>() {
            scheduler.request_refresh();
        }
    }));
    items.push(action_item("Quit", |tray: &mut MeterTray<R>| {
        tray.app.exit(0);
    }));
    items
}

/// A disabled, informational line.
fn info_item<R: Runtime>(label: &str) -> KsniMenuItem<MeterTray<R>> {
    StandardItem {
        label: label.to_owned(),
        enabled: false,
        ..StandardItem::default()
    }
    .into()
}

fn action_item<R: Runtime>(
    label: &str,
    activate: impl Fn(&mut MeterTray<R>) + Send + 'static,
) -> KsniMenuItem<MeterTray<R>> {
    StandardItem {
        label: label.to_owned(),
        activate: Box::new(activate),
        ..StandardItem::default()
    }
    .into()
}

/// Filled in once registration with the `StatusNotifierWatcher` completes;
/// `None` until then — see [`KsniTray::build`].
type HandleSlot<R> = Arc<Mutex<Option<Handle<MeterTray<R>>>>>;

/// The live tray handle the shared core pushes into.
pub(in crate::tray) struct KsniTray<R: Runtime> {
    shared: Arc<Mutex<Shared>>,
    handle: HandleSlot<R>,
    files: IconFiles,
}

impl<R: Runtime> KsniTray<R> {
    /// Make ksni re-read the properties and emit the SNI change signals. The
    /// data is already written; this only notifies. Fire-and-forget because
    /// the caller is synchronous and runs inside the tokio runtime, where
    /// blocking would panic.
    ///
    /// A no-op until registration completes, which loses nothing: the panel
    /// reads every property from the [`Tray`] impl when it first shows the
    /// item, and that already reflects the latest state.
    fn notify(&self) {
        let Some(handle) = lock(&self.handle).clone() else {
            return;
        };
        tauri::async_runtime::spawn(async move {
            handle.update(|_| ()).await;
        });
    }
}

impl<R: Runtime> TrayBackend<R> for KsniTray<R> {
    fn build(
        app: &AppHandle<R>,
        icon: Option<&RenderedIcon>,
        menu: &MenuModel,
    ) -> tauri::Result<Self> {
        let mut files = IconFiles::new();
        let shared = Arc::new(Mutex::new(Shared {
            icon_name: icon.and_then(|icon| files.write(icon)).unwrap_or_default(),
            icon_theme_path: files.dir(),
            icon: icon.map(to_argb32),
            menu: menu.clone(),
        }));
        let tray = MeterTray {
            app: app.clone(),
            shared: Arc::clone(&shared),
        };

        // Registering is async, and `init` is not. Blocking is not an option:
        // Tauri's setup hook already runs on a tokio worker, so `block_on`
        // here panics with "cannot start a runtime from within a runtime".
        // So register in the background and publish the handle when it lands;
        // `notify` tolerates its absence, and the item's properties are read
        // from `shared`, which is already correct.
        let handle: HandleSlot<R> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&handle);
        tauri::async_runtime::spawn(async move {
            match tray.spawn().await {
                Ok(registered) => *lock(&slot) = Some(registered),
                // Not fatal, and not worth aborting startup for: the app is
                // still usable through its windows, and the message says
                // plainly that the tray is the part that is missing.
                Err(error) => eprintln!("tray registration failed, no tray icon: {error}"),
            }
        });
        Ok(Self {
            shared,
            handle,
            files,
        })
    }

    fn set_icon(&mut self, _app: &AppHandle<R>, icon: &RenderedIcon) -> bool {
        // A failed write leaves the previous name in place, so the panel keeps
        // the last good image rather than blanking.
        let name = self.files.write(icon);
        {
            let mut shared = lock(&self.shared);
            if let Some(name) = name {
                shared.icon_name = name;
            }
            shared.icon = Some(to_argb32(icon));
        }
        self.notify();
        true
    }

    fn set_menu(&mut self, _app: &AppHandle<R>, menu: &MenuModel) -> bool {
        lock(&self.shared).menu = menu.clone();
        self.notify();
        true
    }
}

/// `meter-render` emits straight-alpha RGBA; SNI wants ARGB32 in network byte
/// order. That is a per-pixel byte rotate, nothing more.
fn to_argb32(icon: &RenderedIcon) -> Icon {
    let mut data = icon.rgba.clone();
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }
    Icon {
        width: i32::try_from(icon.width).unwrap_or(0),
        height: i32::try_from(icon.height).unwrap_or(0),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::to_argb32;
    use meter_render::RenderedIcon;
    use pretty_assertions::assert_eq;

    fn icon(rgba: Vec<u8>, width: u32, height: u32) -> RenderedIcon {
        RenderedIcon {
            width,
            height,
            rgba,
            is_template: false,
        }
    }

    #[test]
    fn rgba_becomes_argb_per_pixel() {
        // R=1 G=2 B=3 A=4  ->  A=4 R=1 G=2 B=3
        let converted = to_argb32(&icon(vec![1, 2, 3, 4], 1, 1));
        assert_eq!(converted.data, vec![4, 1, 2, 3]);
    }

    #[test]
    fn every_pixel_is_rotated_independently() {
        let converted = to_argb32(&icon(vec![1, 2, 3, 4, 5, 6, 7, 8], 2, 1));
        assert_eq!(converted.data, vec![4, 1, 2, 3, 8, 5, 6, 7]);
    }

    #[test]
    fn dimensions_carry_through() {
        let converted = to_argb32(&icon(vec![0; 4 * 44 * 44], 44, 44));
        assert_eq!((converted.width, converted.height), (44, 44));
        assert_eq!(converted.data.len(), 4 * 44 * 44);
    }

    #[test]
    fn a_fully_opaque_pixel_stays_opaque() {
        // The alpha byte must land first, or the panel renders the icon as
        // fully transparent (alpha read from what was the red channel).
        let converted = to_argb32(&icon(vec![0x10, 0x20, 0x30, 0xFF], 1, 1));
        assert_eq!(converted.data[0], 0xFF);
    }
}
