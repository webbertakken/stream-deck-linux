//! Error types for the Stream Deck library.

use std::io;

/// Library result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur while talking to a Stream Deck.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No supported Stream Deck device was found.
    #[error("no supported Stream Deck device found")]
    DeviceNotFound,

    /// A key index was out of range for the device.
    #[error("key index {index} out of range (device has {count} keys)")]
    KeyOutOfRange { index: u8, count: u8 },

    /// Underlying OS / hidraw I/O error.
    #[error("hid i/o error: {0}")]
    Io(#[from] io::Error),

    /// Image decoding/encoding error.
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    /// Config file could not be parsed.
    #[error("config parse error: {0}")]
    ConfigParse(#[from] toml::de::Error),

    /// Config file is structurally valid but semantically wrong.
    #[error("invalid config: {0}")]
    ConfigInvalid(String),

    /// System tray (StatusNotifierItem) error.
    #[error("tray error: {0}")]
    Tray(String),

    /// Web UI server error.
    #[error("web ui error: {0}")]
    Web(String),
}
