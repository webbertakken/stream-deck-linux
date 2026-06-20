//! Compose a key's visual: a background (image or colour) with an optional
//! centred text label, then encode it for upload.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{DynamicImage, Rgb, RgbImage};

use crate::error::Result;
use crate::image as key_image;
use crate::model::ImageSpec;

/// Bundled fallback font (SIL OFL 1.1, see `assets/LiberationSans-LICENSE.txt`).
const FONT_BYTES: &[u8] = include_bytes!("../assets/LiberationSans-Regular.ttf");

/// Largest fraction of the key height used as the initial text size.
const TEXT_HEIGHT_FRACTION: f32 = 0.34;
/// Horizontal fraction of the key the text must fit within.
const TEXT_WIDTH_FRACTION: f32 = 0.92;
/// Smallest text size we will shrink to before giving up on fitting.
const MIN_TEXT_PX: f32 = 8.0;

/// The lazily-parsed bundled font.
pub fn default_font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).expect("bundled font must be valid"))
}

/// A mutable RGB canvas for one key, built upright and finalised on encode.
pub struct KeySurface {
    spec: ImageSpec,
    canvas: RgbImage,
}

impl KeySurface {
    /// A new black surface sized for the model's keys.
    pub fn new(spec: &ImageSpec) -> Self {
        Self {
            spec: *spec,
            canvas: RgbImage::from_pixel(spec.width, spec.height, Rgb([0, 0, 0])),
        }
    }

    /// Fill the whole surface with a solid colour.
    pub fn fill(&mut self, rgb: [u8; 3]) {
        for pixel in self.canvas.pixels_mut() {
            *pixel = Rgb(rgb);
        }
    }

    /// Replace the background with a picture resized to the key.
    pub fn draw_image(&mut self, source: &DynamicImage) {
        self.canvas = source
            .resize_exact(
                self.spec.width,
                self.spec.height,
                image::imageops::FilterType::Lanczos3,
            )
            .to_rgb8();
    }

    /// Draw a single line of text, centred, on top of the current background.
    pub fn draw_text_centered(&mut self, text: &str, color: [u8; 3]) {
        draw_text_centered(&mut self.canvas, text, color, default_font());
    }

    /// The upright composed canvas, before orientation/encoding. Useful for
    /// previews and tests.
    pub fn canvas(&self) -> &RgbImage {
        &self.canvas
    }

    /// Orient for the model and encode to the wire format.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let oriented = key_image::orient(&self.spec, self.canvas.clone());
        key_image::encode_rgb(&self.spec, &oriented)
    }
}

/// Width in pixels of `text` rendered at `px` with the given font.
fn text_width(font: &FontRef, text: &str, px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut previous = None;
    for ch in text.chars() {
        let glyph = font.glyph_id(ch);
        if let Some(prev) = previous {
            width += scaled.kern(prev, glyph);
        }
        width += scaled.h_advance(glyph);
        previous = Some(glyph);
    }
    width
}

/// Pick the largest text size (down to a floor) that fits the canvas width.
fn fit_text_px(canvas: &RgbImage, font: &FontRef, text: &str) -> f32 {
    let max_width = canvas.width() as f32 * TEXT_WIDTH_FRACTION;
    let mut px = canvas.height() as f32 * TEXT_HEIGHT_FRACTION;
    while px > MIN_TEXT_PX && text_width(font, text, px) > max_width {
        px *= 0.92;
    }
    px
}

/// Alpha-blend `color` onto a pixel using glyph coverage in `0.0..=1.0`.
fn blend_pixel(canvas: &mut RgbImage, x: u32, y: u32, color: [u8; 3], coverage: f32) {
    let bg = canvas.get_pixel(x, y).0;
    let a = coverage.clamp(0.0, 1.0);
    let mix = |fg: u8, bg: u8| (fg as f32 * a + bg as f32 * (1.0 - a)).round() as u8;
    canvas.put_pixel(
        x,
        y,
        Rgb([
            mix(color[0], bg[0]),
            mix(color[1], bg[1]),
            mix(color[2], bg[2]),
        ]),
    );
}

fn draw_text_centered(canvas: &mut RgbImage, text: &str, color: [u8; 3], font: &FontRef) {
    if text.is_empty() {
        return;
    }

    let px = fit_text_px(canvas, font, text);
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);

    let (cw, ch) = (canvas.width() as f32, canvas.height() as f32);
    let text_w = text_width(font, text, px);
    let ascent = scaled.ascent();
    let text_h = ascent - scaled.descent();
    let start_x = (cw - text_w) / 2.0;
    let baseline_y = (ch - text_h) / 2.0 + ascent;

    let mut caret = start_x;
    let mut previous = None;
    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        if let Some(prev) = previous {
            caret += scaled.kern(prev, glyph_id);
        }
        let glyph = glyph_id.with_scale_and_position(scale, point(caret, baseline_y));
        caret += scaled.h_advance(glyph_id);
        previous = Some(glyph_id);

        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px_x = bounds.min.x as i32 + gx as i32;
                let px_y = bounds.min.y as i32 + gy as i32;
                if px_x >= 0
                    && px_y >= 0
                    && (px_x as u32) < canvas.width()
                    && (px_y as u32) < canvas.height()
                {
                    blend_pixel(canvas, px_x as u32, px_y as u32, color, coverage);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;

    fn bright_pixel_count(canvas: &RgbImage) -> usize {
        canvas
            .pixels()
            .filter(|p| p.0[0] > 128 && p.0[1] > 128 && p.0[2] > 128)
            .count()
    }

    #[test]
    fn bundled_font_parses() {
        // Panics inside if the embedded font is invalid.
        let _ = default_font();
    }

    #[test]
    fn text_width_grows_with_length() {
        let font = default_font();
        let short = text_width(font, "I", 20.0);
        let long = text_width(font, "IIIII", 20.0);
        assert!(long > short);
        assert!(short > 0.0);
    }

    #[test]
    fn wide_text_shrinks_to_fit() {
        let canvas = RgbImage::new(72, 72);
        let font = default_font();
        let big = fit_text_px(&canvas, font, "Hi");
        let small = fit_text_px(&canvas, font, "WWWWWWWWWW");
        assert!(small < big);
    }

    #[test]
    fn drawing_text_lights_up_pixels() {
        let mut canvas = RgbImage::from_pixel(72, 72, Rgb([0, 0, 0]));
        assert_eq!(bright_pixel_count(&canvas), 0);
        draw_text_centered(&mut canvas, "Hi", [255, 255, 255], default_font());
        assert!(bright_pixel_count(&canvas) > 0, "text should paint pixels");
    }

    #[test]
    fn empty_text_is_a_noop() {
        let mut canvas = RgbImage::from_pixel(72, 72, Rgb([10, 20, 30]));
        draw_text_centered(&mut canvas, "", [255, 255, 255], default_font());
        assert!(canvas.pixels().all(|p| p.0 == [10, 20, 30]));
    }

    #[test]
    fn surface_fill_then_encode_is_decodable() {
        let mut surface = KeySurface::new(&Model::MK2.image);
        surface.fill([0, 128, 255]);
        surface.draw_text_centered("OK", [255, 255, 255]);
        let bytes = surface.encode().unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 72);
        assert_eq!(decoded.height(), 72);
    }

    #[test]
    fn text_centres_within_canvas() {
        // White text on black: the lit pixels' centroid should sit near middle.
        let mut canvas = RgbImage::from_pixel(72, 72, Rgb([0, 0, 0]));
        draw_text_centered(&mut canvas, "X", [255, 255, 255], default_font());
        let (mut sx, mut sy, mut n) = (0.0f32, 0.0f32, 0.0f32);
        for (x, y, p) in canvas.enumerate_pixels() {
            if p.0[0] > 128 {
                sx += x as f32;
                sy += y as f32;
                n += 1.0;
            }
        }
        assert!(n > 0.0);
        let (cx, cy) = (sx / n, sy / n);
        assert!((cx - 36.0).abs() < 12.0, "x centroid {cx} not centred");
        assert!((cy - 36.0).abs() < 12.0, "y centroid {cy} not centred");
    }
}
