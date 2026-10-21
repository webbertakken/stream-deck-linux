//! Compose a key's visual: a background (image or colour) with an optional
//! centred text label, then encode it for upload.

use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{DynamicImage, Rgb, RgbImage};

use crate::config::ButtonConfig;
use crate::error::Result;
use crate::icons::{self, Coverage};
use crate::image as key_image;
use crate::model::ImageSpec;

/// Colour shown on a key whose image failed to load and that has no colour
/// fallback, so a broken tile is loud rather than silent.
pub const ERROR_TILE: [u8; 3] = [255, 0, 255];
/// Default label colour when none is configured.
pub const DEFAULT_TEXT_COLOR: [u8; 3] = [255, 255, 255];

/// Bundled fallback font (SIL OFL 1.1, see `assets/LiberationSans-LICENSE.txt`).
const FONT_BYTES: &[u8] = include_bytes!("../assets/LiberationSans-Regular.ttf");

/// Largest fraction of the key height used as the initial text size.
const TEXT_HEIGHT_FRACTION: f32 = 0.34;
/// Horizontal fraction of the key the text must fit within.
const TEXT_WIDTH_FRACTION: f32 = 0.92;
/// Smallest text size we will shrink to before giving up on fitting.
const MIN_TEXT_PX: f32 = 8.0;

/// Fraction of the key an icon fills when it is the key's only sign.
const ICON_ALONE_FRACTION: f32 = 0.66;
/// Fraction of the key an icon fills above a label.
const ICON_WITH_LABEL_FRACTION: f32 = 0.5;
/// Largest label size under an icon, as a fraction of the key height.
const LABEL_UNDER_ICON_FRACTION: f32 = 0.2;
/// Space between an icon and its label, as a fraction of the key height.
const ICON_LABEL_GAP_FRACTION: f32 = 0.04;
/// A label's line height, as a multiple of its text size.
const LINE_HEIGHT: f32 = 1.25;

/// Where a line of text goes: a horizontal band, and the largest size to try.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBand {
    pub top: f32,
    pub height: f32,
    pub max_px: f32,
}

/// Where an icon goes on a key, and its label's band when it has one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconLayout {
    pub icon_x: u32,
    pub icon_y: u32,
    pub icon_size: u32,
    pub label: Option<TextBand>,
}

/// Lucide draws 2px strokes on a 24px grid: sizes in steps of 12 pixels keep
/// every stroke on whole pixels, so icons stay sharp on a small key.
fn crisp_icon_size(px: f32) -> u32 {
    ((px / 12.0).round() as u32).max(1) * 12
}

/// Lay an icon out on a `width` x `height` key: alone, centred and large;
/// with a label, smaller, with the label below it, the pair centred.
pub fn icon_layout(width: u32, height: u32, with_label: bool) -> IconLayout {
    let h = height as f32;
    let centred_x = |size: u32| width.saturating_sub(size) / 2;
    if !with_label {
        let size = crisp_icon_size(h * ICON_ALONE_FRACTION);
        return IconLayout {
            icon_x: centred_x(size),
            icon_y: height.saturating_sub(size) / 2,
            icon_size: size,
            label: None,
        };
    }
    let size = crisp_icon_size(h * ICON_WITH_LABEL_FRACTION);
    let max_px = h * LABEL_UNDER_ICON_FRACTION;
    let band_height = max_px * LINE_HEIGHT;
    let gap = (h * ICON_LABEL_GAP_FRACTION).round();
    let top = ((h - (size as f32 + gap + band_height)) / 2.0)
        .round()
        .max(0.0);
    IconLayout {
        icon_x: centred_x(size),
        icon_y: top as u32,
        icon_size: size,
        label: Some(TextBand {
            top: top + size as f32 + gap,
            height: band_height,
            max_px,
        }),
    }
}

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

    /// Draw a single line of text, centred across, within `band`.
    pub fn draw_text_in_band(&mut self, text: &str, color: [u8; 3], band: TextBand) {
        draw_text_in_band(&mut self.canvas, text, color, default_font(), band);
    }

    /// Paint `color` through an icon's coverage, its top-left at `(x, y)`.
    pub fn draw_coverage(&mut self, coverage: &Coverage, x: u32, y: u32, color: [u8; 3]) {
        let size = coverage.size;
        for (index, alpha) in coverage.alpha.iter().enumerate() {
            let (px, py) = (x + index as u32 % size, y + index as u32 / size);
            if *alpha > 0 && px < self.canvas.width() && py < self.canvas.height() {
                blend_pixel(&mut self.canvas, px, py, color, *alpha as f32 / 255.0);
            }
        }
    }

    /// Blend every pixel toward white by `amount` (0..=1) for a pressed-key
    /// highlight.
    pub fn brighten(&mut self, amount: f32) {
        let a = amount.clamp(0.0, 1.0);
        for pixel in self.canvas.pixels_mut() {
            for c in 0..3 {
                let v = pixel.0[c] as f32;
                pixel.0[c] = (v + (255.0 - v) * a).round() as u8;
            }
        }
    }

    /// The upright composed canvas, before orientation/encoding. Useful for
    /// previews and tests.
    pub fn canvas(&self) -> &RgbImage {
        &self.canvas
    }

    /// The upright composed canvas, taken whole.
    pub fn into_canvas(self) -> RgbImage {
        self.canvas
    }

    /// Orient for the model and encode to the wire format.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let oriented = key_image::orient(&self.spec, self.canvas.clone());
        key_image::encode_rgb(&self.spec, &oriented)
    }
}

/// Compose a key's visual from its config: background (image or colour), an
/// optional icon, and an optional label (centred, or under the icon). Image
/// and icon failures are reported and shown as the colour or an error tile,
/// never a silent blank.
pub fn compose(spec: &ImageSpec, base_dir: &Path, button: &ButtonConfig) -> KeySurface {
    let mut surface = KeySurface::new(spec);

    let drew_image = match button.resolved_image(base_dir) {
        Some(path) => match image::open(&path) {
            Ok(picture) => {
                surface.draw_image(&picture);
                true
            }
            Err(err) => {
                eprintln!(
                    "warning: key {} image '{}' failed to load: {err}",
                    button.key,
                    path.display()
                );
                false
            }
        },
        None => false,
    };

    if !drew_image {
        match button.rgb() {
            Ok(Some(rgb)) => surface.fill(rgb),
            Ok(None) if button.label.is_some() || button.icon.is_some() => {} // keep black
            _ => surface.fill(ERROR_TILE),
        }
    }

    let color = button
        .text_rgb()
        .ok()
        .flatten()
        .unwrap_or(DEFAULT_TEXT_COLOR);
    let label_band = match &button.icon {
        Some(name) => draw_icon(&mut surface, spec, name, button, color),
        None => None,
    };

    if let Some(label) = &button.label {
        match label_band {
            Some(band) => surface.draw_text_in_band(label, color, band),
            None => surface.draw_text_centered(label, color),
        }
    }

    surface
}

/// Draw the icon called `name`, answering the band its label goes in. An
/// icon that cannot be drawn turns the key into the error tile.
fn draw_icon(
    surface: &mut KeySurface,
    spec: &ImageSpec,
    name: &str,
    button: &ButtonConfig,
    color: [u8; 3],
) -> Option<TextBand> {
    let layout = icon_layout(spec.width, spec.height, button.label.is_some());
    let drawn = icons::resolve(name)
        .and_then(|icon| icons::rasterise(icon, layout.icon_size))
        .map(|coverage| surface.draw_coverage(&coverage, layout.icon_x, layout.icon_y, color));
    match drawn {
        Ok(()) => layout.label,
        Err(why) => {
            eprintln!("warning: key {} icon: {why}", button.key);
            surface.fill(ERROR_TILE);
            None
        }
    }
}

/// Compose a key and encode it as a PNG (upright) for previews.
pub fn button_png(spec: &ImageSpec, base_dir: &Path, button: &ButtonConfig) -> Result<Vec<u8>> {
    let surface = compose(spec, base_dir, button);
    let mut buffer = Vec::new();
    DynamicImage::ImageRgb8(surface.canvas().clone())
        .write_to(&mut Cursor::new(&mut buffer), image::ImageFormat::Png)?;
    Ok(buffer)
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

/// Pick the largest text size from `max_px` (down to a floor) that fits the
/// canvas width.
fn fit_text_px(canvas: &RgbImage, font: &FontRef, text: &str, max_px: f32) -> f32 {
    let max_width = canvas.width() as f32 * TEXT_WIDTH_FRACTION;
    let mut px = max_px;
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
    let height = canvas.height() as f32;
    let band = TextBand {
        top: 0.0,
        height,
        max_px: height * TEXT_HEIGHT_FRACTION,
    };
    draw_text_in_band(canvas, text, color, font, band);
}

fn draw_text_in_band(
    canvas: &mut RgbImage,
    text: &str,
    color: [u8; 3],
    font: &FontRef,
    band: TextBand,
) {
    if text.is_empty() {
        return;
    }

    let px = fit_text_px(canvas, font, text, band.max_px);
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);

    let cw = canvas.width() as f32;
    let text_w = text_width(font, text, px);
    let ascent = scaled.ascent();
    let text_h = ascent - scaled.descent();
    let start_x = (cw - text_w) / 2.0;
    let baseline_y = band.top + (band.height - text_h) / 2.0 + ascent;

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
        let big = fit_text_px(&canvas, font, "Hi", 24.0);
        let small = fit_text_px(&canvas, font, "WWWWWWWWWW", 24.0);
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
    fn brighten_blends_toward_white() {
        let mut surface = KeySurface::new(&Model::MK2.image);
        surface.fill([0, 100, 254]);
        surface.brighten(0.5);
        let p = surface.canvas().get_pixel(0, 0).0;
        assert_eq!(p[0], 128); // 0 + 255*0.5
        assert_eq!(p[1], 178); // 100 + 155*0.5
        assert_eq!(p[2], 255); // 254 + 1*0.5 -> 254.5 -> 255
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

    fn icon_key(icon: &str, label: Option<&str>) -> ButtonConfig {
        ButtonConfig {
            key: 0,
            icon: Some(icon.into()),
            label: label.map(String::from),
            color: Some("#2E5A88".into()),
            ..Default::default()
        }
    }

    /// The bounding box `(min_x, min_y, max_x, max_y)` of pixels unlike `background`.
    fn ink_box(canvas: &RgbImage, background: [u8; 3]) -> Option<(u32, u32, u32, u32)> {
        let inked = canvas
            .enumerate_pixels()
            .filter(|(_, _, p)| p.0 != background);
        inked.fold(None, |acc, (x, y, _)| match acc {
            None => Some((x, y, x, y)),
            Some((a, b, c, d)) => Some((a.min(x), b.min(y), c.max(x), d.max(y))),
        })
    }

    const APPS: [u8; 3] = [0x2E, 0x5A, 0x88];

    #[test]
    fn lays_an_icon_out_alone_larger_and_above_a_label_smaller() {
        let alone = icon_layout(72, 72, false);
        let stacked = icon_layout(72, 72, true);
        assert!(alone.label.is_none());
        assert!(alone.icon_size > stacked.icon_size);
        assert_eq!(alone.icon_x * 2 + alone.icon_size, 72, "centred across");
        assert_eq!(alone.icon_y * 2 + alone.icon_size, 72, "centred down");
        assert_eq!(stacked.icon_x * 2 + stacked.icon_size, 72);
        let band = stacked.label.unwrap();
        assert!((stacked.icon_y + stacked.icon_size) as f32 <= band.top);
        assert!(band.top + band.height <= 72.0);
        assert!(band.max_px < 72.0 * TEXT_HEIGHT_FRACTION, "a smaller label");
        assert_eq!(
            (alone.icon_size, stacked.icon_size),
            (48, 36),
            "whole-pixel strokes"
        );
    }

    #[test]
    fn draws_an_icon_tinted_in_white_over_the_key_colour() {
        let canvas =
            compose(&Model::MK2.image, Path::new("."), &icon_key("mail", None)).into_canvas();
        assert_eq!(
            canvas.get_pixel(0, 0).0,
            APPS,
            "the corner keeps the colour"
        );
        let white = canvas.pixels().filter(|p| p.0 == [255, 255, 255]).count();
        assert!(white > 100, "only {white} white pixels");
        let layout = icon_layout(72, 72, false);
        let (x0, y0, x1, y1) = ink_box(&canvas, APPS).unwrap();
        assert!(x0 >= layout.icon_x && y0 >= layout.icon_y);
        assert!(x1 < layout.icon_x + layout.icon_size && y1 < layout.icon_y + layout.icon_size);
        assert!(x1 - x0 > layout.icon_size / 2, "the icon fills its box");
    }

    #[test]
    fn tints_an_icon_in_the_text_colour() {
        let mut button = icon_key("square", None);
        button.text_color = Some("#FFCC00".into());
        let canvas = compose(&Model::MK2.image, Path::new("."), &button).into_canvas();
        assert!(
            canvas
                .pixels()
                .filter(|p| p.0 == [0xFF, 0xCC, 0x00])
                .count()
                > 100
        );
        assert!(!canvas.pixels().any(|p| p.0 == [255, 255, 255]));
    }

    #[test]
    fn puts_the_label_below_the_icon() {
        let with_label = compose(
            &Model::MK2.image,
            Path::new("."),
            &icon_key("mail", Some("Mail")),
        );
        let canvas = with_label.into_canvas();
        let layout = icon_layout(72, 72, true);
        let band = layout.label.unwrap();
        let icon_bottom = layout.icon_y + layout.icon_size;
        let label_ink = canvas
            .enumerate_pixels()
            .filter(|(_, y, p)| *y >= icon_bottom && p.0 != APPS)
            .count();
        assert!(label_ink > 20, "the label is drawn below the icon");
        let (_, y0, _, y1) = ink_box(&canvas, APPS).unwrap();
        assert!(y0 >= layout.icon_y && (y1 as f32) < band.top + band.height);
    }

    #[test]
    fn draws_an_icon_on_black_without_a_colour() {
        let mut button = icon_key("mail", None);
        button.color = None;
        let canvas = compose(&Model::MK2.image, Path::new("."), &button).into_canvas();
        assert_eq!(canvas.get_pixel(0, 0).0, [0, 0, 0]);
        assert!(canvas.pixels().any(|p| p.0 == [255, 255, 255]));
    }

    #[test]
    fn shows_an_unknown_icon_as_the_error_tile() {
        let canvas =
            compose(&Model::MK2.image, Path::new("."), &icon_key("mial", None)).into_canvas();
        assert_eq!(canvas.get_pixel(0, 0).0, ERROR_TILE);
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
