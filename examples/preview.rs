//! Render a contact sheet of composed keys to a PNG, for visual inspection
//! without the hardware. Run with:
//!   cargo run --example preview -- /tmp/streamdeck_preview.png

use image::{Rgb, RgbImage};
use streamdeck::model::Model;
use streamdeck::render::KeySurface;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/streamdeck_preview.png".to_string());

    let spec = Model::MK2.image;
    let size = spec.width; // 72
    let (cols, rows) = (5u32, 3u32);
    let gap = 6u32;
    let sheet_w = cols * size + (cols + 1) * gap;
    let sheet_h = rows * size + (rows + 1) * gap;
    let mut sheet = RgbImage::from_pixel(sheet_w, sheet_h, Rgb([24, 24, 30]));

    // (background colour or None for black, label, text colour)
    type Tile = (Option<[u8; 3]>, &'static str, [u8; 3]);
    let tiles: Vec<Tile> = vec![
        (Some([30, 30, 46]), "Term", [255, 255, 255]),
        (Some([0, 0, 0]), "Mute", [255, 255, 255]),
        (Some([204, 34, 34]), "Rec", [255, 255, 255]),
        (Some([34, 119, 204]), "Vol+", [255, 255, 255]),
        (Some([34, 170, 85]), "Go", [0, 0, 0]),
        (Some([170, 34, 170]), "Quit", [255, 255, 255]),
        (Some([60, 60, 60]), "12:45", [120, 220, 120]),
        (Some([0, 0, 0]), "WWWWW", [255, 200, 0]),
        (Some([20, 80, 120]), "OBS", [255, 255, 255]),
        (Some([120, 20, 60]), "Cut", [255, 255, 255]),
        (Some([240, 240, 240]), "Dark", [20, 20, 20]),
        (Some([10, 10, 10]), "Hi!", [80, 200, 255]),
    ];

    for (i, (bg, label, text)) in tiles.iter().enumerate() {
        let mut surface = KeySurface::new(&spec);
        if let Some(rgb) = bg {
            surface.fill(*rgb);
        }
        surface.draw_text_centered(label, *text);

        let col = i as u32 % cols;
        let row = i as u32 / cols;
        let ox = gap + col * (size + gap);
        let oy = gap + row * (size + gap);
        for (x, y, pixel) in surface.canvas().enumerate_pixels() {
            sheet.put_pixel(ox + x, oy + y, *pixel);
        }
    }

    sheet.save(&out).expect("save preview png");
    println!("wrote {out} ({sheet_w}x{sheet_h})");
}
