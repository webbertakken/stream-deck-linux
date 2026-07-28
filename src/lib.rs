//! Custom Linux control library for Elgato Stream Deck devices.
//!
//! Talks to the device directly over `/dev/hidraw*` (no libudev/hidapi
//! dependency). The protocol constants are grounded in the device's own HID
//! report descriptor, not guessed.

pub mod actions;
pub mod apps;
pub mod autostart;
pub mod config;
pub mod device;
pub mod error;
pub mod events;
pub mod hid;
pub mod image;
pub mod install;
pub mod keyboard;
pub mod model;
pub mod protocol;
pub mod render;
pub mod runtime;
pub mod system;
pub mod tray;
pub mod webui;

pub use config::{ButtonConfig, Config};
pub use device::StreamDeck;
pub use error::{Error, Result};
pub use events::{diff_states, KeyEvent, KeyEventKind};
pub use model::Model;
