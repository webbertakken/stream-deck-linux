//! Turn arbitrary pictures into the exact byte payload a key expects.
//!
//! Pipeline: fit the source to the key's pixel size, orient it the way the
//! hardware wants (the MK.2 shows images rotated 180 degrees), then encode to
//! the model's image format.

use image::{DynamicImage, Rgb, RgbImage};

use crate::error::Result;
use crate::model::{ImageFormat, ImageSpec};

/// Default JPEG quality for key images. High enough to look crisp on the small
/// 72x72 panels without bloating each upload.
const JPEG_QUALITY: u8 = 90;

/// Resize a source image to exactly the key's pixel dimensions (RGB8).
fn fit(spec: &ImageSpec, source: &DynamicImage) -> RgbImage {
    source
        .resize_exact(
            spec.width,
            spec.height,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgb8()
}

/// Apply the model's required flips and rotation to a fitted image.
pub fn orient(spec: &ImageSpec, mut img: RgbImage) -> RgbImage {
    if spec.orientation.flip_horizontal {
        image::imageops::flip_horizontal_in_place(&mut img);
    }
    if spec.orientation.flip_vertical {
        image::imageops::flip_vertical_in_place(&mut img);
    }
    for _ in 0..(spec.orientation.rotate_quarter_turns % 4) {
        img = image::imageops::rotate90(&img);
    }
    img
}

/// Encode an oriented RGB image to the model's wire format.
pub fn encode_rgb(spec: &ImageSpec, img: &RgbImage) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    match spec.format {
        ImageFormat::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, JPEG_QUALITY)
                .encode_image(img)?;
        }
    }
    Ok(buffer)
}

/// Convert a source picture into the encoded payload for a single key.
pub fn encode_key_image(spec: &ImageSpec, source: &DynamicImage) -> Result<Vec<u8>> {
    let fitted = fit(spec, source);
    let oriented = orient(spec, fitted);
    encode_rgb(spec, &oriented)
}

/// Build the encoded payload for a key filled with a single solid colour.
pub fn solid_color(spec: &ImageSpec, rgb: [u8; 3]) -> Result<Vec<u8>> {
    let img = RgbImage::from_pixel(spec.width, spec.height, Rgb(rgb));
    // Solid colours are orientation-invariant, but keep the pipeline uniform.
    let oriented = orient(spec, img);
    encode_rgb(spec, &oriented)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Model, Orientation};

    fn spec_no_orientation() -> ImageSpec {
        ImageSpec {
            width: 2,
            height: 2,
            format: ImageFormat::Jpeg,
            orientation: Orientation {
                flip_horizontal: false,
                flip_vertical: false,
                rotate_quarter_turns: 0,
            },
        }
    }

    #[test]
    fn orient_flips_both_axes_for_mk2() {
        // Distinct pixels so we can track corners after a 180-degree flip.
        let mut img = RgbImage::new(2, 2);
        img.put_pixel(0, 0, Rgb([1, 1, 1])); // top-left
        img.put_pixel(1, 0, Rgb([2, 2, 2])); // top-right
        img.put_pixel(0, 1, Rgb([3, 3, 3])); // bottom-left
        img.put_pixel(1, 1, Rgb([4, 4, 4])); // bottom-right

        let oriented = orient(&Model::MK2.image, img);

        // Flip H + flip V swaps opposite corners (== 180-degree rotation).
        assert_eq!(oriented.get_pixel(0, 0), &Rgb([4, 4, 4]));
        assert_eq!(oriented.get_pixel(1, 0), &Rgb([3, 3, 3]));
        assert_eq!(oriented.get_pixel(0, 1), &Rgb([2, 2, 2]));
        assert_eq!(oriented.get_pixel(1, 1), &Rgb([1, 1, 1]));
    }

    #[test]
    fn orient_is_identity_when_unset() {
        let mut img = RgbImage::new(2, 2);
        img.put_pixel(0, 0, Rgb([9, 8, 7]));
        let oriented = orient(&spec_no_orientation(), img.clone());
        assert_eq!(oriented, img);
    }

    #[test]
    fn encode_key_image_fits_to_key_dimensions() {
        // A 10x10 source must come back out at the MK.2's 72x72.
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(10, 10, Rgb([200, 50, 50])));
        let bytes = encode_key_image(&Model::MK2.image, &source).unwrap();
        assert!(!bytes.is_empty());

        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 72);
        assert_eq!(decoded.height(), 72);
    }

    #[test]
    fn solid_color_encodes_to_expected_size() {
        let bytes = solid_color(&Model::MK2.image, [0, 128, 255]).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (72, 72));

        // JPEG is lossy; assert the centre pixel is in the right ballpark.
        let centre = decoded.get_pixel(36, 36);
        assert!(centre[0] < 40, "red channel ~0, got {}", centre[0]);
        assert!(
            (100..=160).contains(&centre[1]),
            "green ~128, got {}",
            centre[1]
        );
        assert!(centre[2] > 215, "blue ~255, got {}", centre[2]);
    }
}
