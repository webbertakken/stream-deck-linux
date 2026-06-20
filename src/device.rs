//! High-level Stream Deck device: discovery and the operations a user cares
//! about - brightness, key images, button reads.

use std::path::{Path, PathBuf};
use std::time::Duration;

use image::DynamicImage;

use crate::error::{Error, Result};
use crate::hid::{self, RawHidDevice};
use crate::image as key_image;
use crate::model::{Model, ELGATO_VENDOR_ID};
use crate::protocol;

/// Feature report ids used to read device identity strings.
const FIRMWARE_REPORT_ID: u8 = 0x05;
const SERIAL_REPORT_ID: u8 = 0x06;
const IDENTITY_REPORT_LEN: usize = 32;
/// Offsets into the identity feature reports where the ASCII string begins.
const FIRMWARE_STRING_OFFSET: usize = 6;
const SERIAL_STRING_OFFSET: usize = 2;

/// A connected, ready-to-drive Stream Deck.
pub struct StreamDeck {
    raw: RawHidDevice,
    model: Model,
    path: PathBuf,
}

impl StreamDeck {
    /// List every connected, supported Stream Deck (path + recognised model).
    pub fn list() -> Result<Vec<(PathBuf, Model)>> {
        let mut decks = Vec::new();
        for (path, info) in hid::enumerate()? {
            if info.vendor_id != ELGATO_VENDOR_ID {
                continue;
            }
            if let Some(model) = Model::from_product_id(info.product_id) {
                decks.push((path, model));
            }
        }
        Ok(decks)
    }

    /// Open the first connected, supported Stream Deck.
    pub fn open_first() -> Result<Self> {
        let (path, _) = Self::list()?
            .into_iter()
            .next()
            .ok_or(Error::DeviceNotFound)?;
        Self::open_path(path)
    }

    /// Open a specific hidraw node, verifying it is a supported Stream Deck.
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let raw = RawHidDevice::open(&path)?;
        let info = raw.info()?;
        if info.vendor_id != ELGATO_VENDOR_ID {
            return Err(Error::DeviceNotFound);
        }
        let model = Model::from_product_id(info.product_id).ok_or(Error::DeviceNotFound)?;
        Ok(Self { raw, model, path })
    }

    /// The recognised hardware model.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The hidraw node path this device was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Device firmware version string.
    pub fn firmware_version(&self) -> Result<String> {
        let report = self
            .raw
            .get_feature(FIRMWARE_REPORT_ID, IDENTITY_REPORT_LEN)?;
        Ok(extract_string(&report[FIRMWARE_STRING_OFFSET..]))
    }

    /// Device serial number string.
    pub fn serial_number(&self) -> Result<String> {
        let report = self
            .raw
            .get_feature(SERIAL_REPORT_ID, IDENTITY_REPORT_LEN)?;
        Ok(extract_string(&report[SERIAL_STRING_OFFSET..]))
    }

    /// Set display brightness as a percentage (`0..=100`).
    pub fn set_brightness(&self, percent: u8) -> Result<()> {
        self.raw
            .send_feature(&protocol::brightness_feature(percent))?;
        Ok(())
    }

    /// Reset the device back to its standby logo.
    pub fn reset(&self) -> Result<()> {
        self.raw.send_feature(&protocol::reset_feature())?;
        Ok(())
    }

    /// Upload an already-encoded key image (model's wire format).
    pub fn set_key_image(&mut self, key: u8, encoded: &[u8]) -> Result<()> {
        self.ensure_key(key)?;
        for packet in protocol::image_packets(key, encoded) {
            self.raw.write_output(&packet)?;
        }
        Ok(())
    }

    /// Fill a key with a solid RGB colour.
    pub fn set_key_color(&mut self, key: u8, rgb: [u8; 3]) -> Result<()> {
        self.ensure_key(key)?;
        let encoded = key_image::solid_color(&self.model.image, rgb)?;
        self.set_key_image(key, &encoded)
    }

    /// Render an arbitrary picture onto a key (resized + oriented for the model).
    pub fn set_key_picture(&mut self, key: u8, source: &DynamicImage) -> Result<()> {
        self.ensure_key(key)?;
        let encoded = key_image::encode_key_image(&self.model.image, source)?;
        self.set_key_image(key, &encoded)
    }

    /// Blank a key (solid black).
    pub fn clear_key(&mut self, key: u8) -> Result<()> {
        self.set_key_color(key, [0, 0, 0])
    }

    /// Blank every key on the device.
    pub fn clear_all(&mut self) -> Result<()> {
        for key in 0..self.model.key_count {
            self.clear_key(key)?;
        }
        Ok(())
    }

    /// Read the latest button states.
    ///
    /// With a timeout, returns `Ok(None)` when no report arrived in time.
    /// Otherwise blocks until the device reports a state change.
    pub fn read_button_states(&mut self, timeout: Option<Duration>) -> Result<Option<Vec<bool>>> {
        let mut buf = [0u8; protocol::INPUT_REPORT_LEN];
        let n = self.raw.read_timeout(&mut buf, timeout)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(protocol::parse_key_states(
            &buf[..n],
            self.model.key_count as usize,
        )))
    }

    fn ensure_key(&self, key: u8) -> Result<()> {
        if self.model.is_valid_key(key) {
            Ok(())
        } else {
            Err(Error::KeyOutOfRange {
                index: key,
                count: self.model.key_count,
            })
        }
    }
}

/// Extract a NUL/whitespace-trimmed ASCII string from a feature report slice.
fn extract_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_string_stops_at_nul_and_trims() {
        assert_eq!(extract_string(b"AB12\0\0\0junk"), "AB12");
        assert_eq!(extract_string(b"  3.10.5  "), "3.10.5");
        assert_eq!(extract_string(b""), "");
    }
}
