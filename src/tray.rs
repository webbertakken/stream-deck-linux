//! System tray (StatusNotifierItem) via the pure-Rust `ksni` crate.
//!
//! The tray runs on the main thread; menu actions hand work to the device
//! daemon over a channel (never block in a menu callback).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Tray};

use crate::error::{Error, Result};
use crate::runtime::Control;

/// Tray icon source (the generated app icon at a tray-friendly size).
const ICON_PNG: &[u8] = include_bytes!("../assets/icons/streamdeck-64.png");

/// Tray model: forwards menu actions to the daemon and the shutdown flag.
pub struct StreamDeckTray {
    control: Sender<Control>,
    shutdown: &'static AtomicBool,
    editor_url: String,
    status: String,
}

impl StreamDeckTray {
    pub fn new(
        control: Sender<Control>,
        shutdown: &'static AtomicBool,
        editor_url: String,
        status: String,
    ) -> Self {
        Self {
            control,
            shutdown,
            editor_url,
            status,
        }
    }

    /// Show the tray (spawns ksni's background D-Bus loop). Keep the returned
    /// handle alive for as long as the tray should be visible.
    pub fn show(self) -> Result<ksni::blocking::Handle<Self>> {
        use ksni::blocking::TrayMethods;
        self.spawn().map_err(|err| Error::Tray(err.to_string()))
    }

    fn send(&self, control: Control) {
        let _ = self.control.send(control);
    }
}

/// Decode the embedded PNG into a ksni ARGB32 (network byte order) icon.
fn app_icon() -> Icon {
    let rgba = image::load_from_memory(ICON_PNG)
        .expect("embedded tray icon must decode")
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        data.extend_from_slice(&[a, r, g, b]);
    }
    Icon {
        width: width as i32,
        height: height as i32,
        data,
    }
}

/// Open a URL in the user's default browser.
fn open_url(url: &str) {
    if let Err(err) = std::process::Command::new("xdg-open").arg(url).spawn() {
        eprintln!("error: could not open {url}: {err}");
    }
}

impl Tray for StreamDeckTray {
    fn id(&self) -> String {
        "stream-deck-linux".into()
    }

    fn title(&self) -> String {
        "Stream Deck".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![app_icon()]
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: self.status.clone(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open editor".into(),
                activate: Box::new(|this: &mut Self| open_url(&this.editor_url)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Reload config".into(),
                activate: Box::new(|this: &mut Self| this.send(Control::Reload)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Brightness +".into(),
                activate: Box::new(|this: &mut Self| this.send(Control::BrightnessUp)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Brightness -".into(),
                activate: Box::new(|this: &mut Self| this.send(Control::BrightnessDown)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Reset device".into(),
                activate: Box::new(|this: &mut Self| this.send(Control::Reset)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|this: &mut Self| {
                    this.shutdown.store(true, Ordering::Relaxed);
                    this.send(Control::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}
