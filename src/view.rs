//! What the device shows right now.
//!
//! The daemon keeps one [`Frame`]: each key's image exactly as it last sent it
//! (upright, before the device's orientation and encoding), the page on show,
//! brightness and standby. It shares the frame through a [`LiveView`], which
//! the control API reads to answer `GET /api/v1/view`.

use std::sync::{Arc, Mutex, MutexGuard};

use base64::Engine;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};
use serde_json::{json, Value};

use crate::error::Result;
use crate::model::Model;

/// The page on show: its 0-based index and its name, if it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRef {
    pub index: usize,
    pub name: Option<String>,
}

/// One key as shown: the upright image last sent to it, and whether its hold
/// latch was on when it was drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyFrame {
    pub image: RgbImage,
    pub held: bool,
}

/// Everything the device shows, as the daemon last drew it.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub model: Model,
    pub page: PageRef,
    pub pages: usize,
    /// The brightness the daemon last set; `None` while it has set none.
    pub brightness: Option<u8>,
    /// The device shows its standby logo (drawn here as blank keys) on every
    /// key not drawn since the reset.
    pub standby: bool,
    /// One per hardware key, in index order.
    pub keys: Vec<KeyFrame>,
}

impl Frame {
    /// Every key blank, on `page` of `pages`.
    pub fn new(model: &Model, page: PageRef, pages: usize) -> Self {
        Self {
            model: *model,
            page,
            pages,
            brightness: None,
            standby: false,
            keys: vec![blank_key(model); model.key_count as usize],
        }
    }

    /// Record the image just sent to `key`. A key the model lacks is ignored:
    /// the device refuses it before anything is sent.
    pub fn show_key(&mut self, key: u8, image: RgbImage, held: bool) {
        if let Some(slot) = self.keys.get_mut(key as usize) {
            *slot = KeyFrame { image, held };
        }
    }

    /// Every key blank, as after clearing the whole device.
    pub fn blank_all(&mut self) {
        let blank = blank_key(&self.model);
        self.keys.iter_mut().for_each(|key| *key = blank.clone());
        self.standby = false;
    }

    /// The device was reset to its standby logo.
    pub fn enter_standby(&mut self) {
        self.blank_all();
        self.standby = true;
    }

    /// The daemon switched to `page` of `pages`.
    pub fn show_page(&mut self, page: PageRef, pages: usize) {
        self.page = page;
        self.pages = pages;
    }

    /// The daemon set the device's brightness.
    pub fn set_brightness(&mut self, percent: u8) {
        self.brightness = Some(percent);
    }

    /// The frame as the control API answers it, each key a base64 PNG.
    pub fn to_json(&self) -> Result<Value> {
        let keys = self
            .keys
            .iter()
            .enumerate()
            .map(|(index, key)| {
                Ok(json!({
                    "key": index,
                    "png": base64::engine::general_purpose::STANDARD.encode(png(&key.image)?),
                    "held": key.held,
                }))
            })
            .collect::<Result<Vec<Value>>>()?;
        Ok(json!({
            "page": { "index": self.page.index, "name": self.page.name },
            "pages": self.pages,
            "model": {
                "name": self.model.name,
                "columns": self.model.columns,
                "rows": self.model.rows,
                "keyCount": self.model.key_count,
            },
            "brightness": self.brightness,
            "standby": self.standby,
            "keys": keys,
        }))
    }
}

fn blank_key(model: &Model) -> KeyFrame {
    KeyFrame {
        image: RgbImage::from_pixel(model.image.width, model.image.height, Rgb([0, 0, 0])),
        held: false,
    }
}

/// Encode an upright RGB image as PNG.
pub fn png(image: &RgbImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        ExtendedColorType::Rgb8,
    )?;
    Ok(bytes)
}

/// The daemon's [`Frame`], shared between the thread that draws and the
/// threads that read. Empty until the daemon publishes its first frame.
#[derive(Debug, Clone, Default)]
pub struct LiveView {
    frame: Arc<Mutex<Option<Frame>>>,
}

impl LiveView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the frame whole.
    pub fn publish(&self, frame: Frame) {
        *self.lock() = Some(frame);
    }

    /// Change the published frame in place; nothing before the first publish.
    pub fn update(&self, change: impl FnOnce(&mut Frame)) {
        if let Some(frame) = self.lock().as_mut() {
            change(frame);
        }
    }

    /// A copy of the frame, so readers encode without holding the lock.
    pub fn snapshot(&self) -> Option<Frame> {
        self.lock().clone()
    }

    /// The lock, recovered if a holder panicked: every change is a whole
    /// assignment, so the frame is never left half-written.
    fn lock(&self) -> MutexGuard<'_, Option<Frame>> {
        self.frame
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(index: usize, name: Option<&str>) -> PageRef {
        PageRef {
            index,
            name: name.map(String::from),
        }
    }

    fn solid(rgb: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(72, 72, Rgb(rgb))
    }

    fn decode(png_base64: &Value) -> RgbImage {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(png_base64.as_str().unwrap())
            .unwrap();
        image::load_from_memory(&bytes).unwrap().to_rgb8()
    }

    #[test]
    fn starts_with_every_key_blank_on_its_page() {
        let frame = Frame::new(&Model::MK2, page(0, None), 1);
        assert_eq!(frame.keys.len(), 15);
        assert!(frame
            .keys
            .iter()
            .all(|k| k.image == solid([0, 0, 0]) && !k.held));
        assert_eq!((frame.brightness, frame.standby), (None, false));
    }

    #[test]
    fn records_a_key_and_ignores_one_the_model_lacks() {
        let mut frame = Frame::new(&Model::MK2, page(0, None), 1);
        frame.show_key(3, solid([1, 2, 3]), true);
        frame.show_key(15, solid([9, 9, 9]), false);
        assert_eq!(
            frame.keys[3],
            KeyFrame {
                image: solid([1, 2, 3]),
                held: true
            }
        );
        assert_eq!(frame.keys.len(), 15);
    }

    #[test]
    fn standby_blanks_every_key_until_the_next_full_clear() {
        let mut frame = Frame::new(&Model::MK2, page(0, None), 1);
        frame.show_key(3, solid([1, 2, 3]), true);
        frame.enter_standby();
        assert!(frame.standby);
        assert!(frame
            .keys
            .iter()
            .all(|k| k.image == solid([0, 0, 0]) && !k.held));
        frame.show_key(4, solid([5, 5, 5]), false);
        assert!(frame.standby, "one key drawn leaves the rest on the logo");
        frame.blank_all();
        assert!(!frame.standby);
    }

    #[test]
    fn answers_the_documented_shape_with_upright_pngs() {
        let mut frame = Frame::new(&Model::MK2, page(1, Some("Media")), 2);
        frame.set_brightness(40);
        let mut marked = solid([10, 20, 30]);
        marked.put_pixel(0, 0, Rgb([255, 0, 0]));
        frame.show_key(7, marked.clone(), true);

        let body = frame.to_json().unwrap();

        assert_eq!(body["page"], json!({ "index": 1, "name": "Media" }));
        assert_eq!(body["pages"], 2);
        assert_eq!(
            body["model"],
            json!({ "name": "Stream Deck MK.2", "columns": 5, "rows": 3, "keyCount": 15 })
        );
        assert_eq!(
            (body["brightness"].as_u64(), body["standby"].as_bool()),
            (Some(40), Some(false))
        );
        let keys = body["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 15);
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(key["key"], index);
            assert_eq!(key.as_object().unwrap().len(), 3, "{key}");
        }
        assert_eq!(keys[7]["held"], true);
        assert_eq!(
            decode(&keys[7]["png"]),
            marked,
            "PNG is lossless and upright"
        );
        assert_eq!(decode(&keys[0]["png"]), solid([0, 0, 0]));
    }

    #[test]
    fn answers_null_brightness_and_name_while_unset() {
        let body = Frame::new(&Model::MK2, page(0, None), 1).to_json().unwrap();
        assert!(body["brightness"].is_null() && body["page"]["name"].is_null());
    }

    #[test]
    fn shares_one_frame_and_is_empty_until_published() {
        let view = LiveView::new();
        let reader = view.clone();
        view.update(|f| f.set_brightness(10));
        assert_eq!(reader.snapshot(), None);

        view.publish(Frame::new(&Model::MK2, page(0, None), 1));
        view.update(|f| f.set_brightness(10));
        assert_eq!(reader.snapshot().unwrap().brightness, Some(10));
    }
}
