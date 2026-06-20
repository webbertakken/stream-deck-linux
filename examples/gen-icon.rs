//! Generate the application icon: a dark rounded panel with a 5x3 grid of
//! colourful keys (a stylised Stream Deck). Pure-Rust, no external tools.
//!
//! Run with: `cargo run --example gen-icon` (writes PNGs into `assets/icons/`).

use image::{Rgba, RgbaImage};

const PANEL: [u8; 3] = [22, 22, 28];
const PANEL_EDGE: [u8; 3] = [44, 44, 54];
const KEY_COLORS: [[u8; 3]; 15] = [
    [0xE6, 0x39, 0x46],
    [0xF3, 0x72, 0x2C],
    [0xF8, 0xCB, 0x2E],
    [0x8A, 0xC9, 0x26],
    [0x2A, 0x9D, 0x8F],
    [0x3A, 0x86, 0xFF],
    [0x83, 0x38, 0xEC],
    [0xFF, 0x00, 0x6E],
    [0x06, 0xD6, 0xA0],
    [0xEF, 0x47, 0x6F],
    [0x11, 0x8A, 0xB2],
    [0xFF, 0xB7, 0x03],
    [0x7B, 0x2C, 0xBF],
    [0x43, 0xAA, 0x8B],
    [0xF9, 0x84, 0x04],
];

/// Signed distance from a point to a rounded rectangle (negative inside).
fn rounded_rect_sdf(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

/// Alpha-blend `color` (with `alpha` 0..1) over an existing pixel.
fn blend(dst: &mut Rgba<u8>, color: [u8; 3], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    for (c, &channel) in color.iter().enumerate() {
        dst.0[c] = (channel as f32 * a + dst.0[c] as f32 * inv).round() as u8;
    }
    dst.0[3] = (a * 255.0).max(dst.0[3] as f32).round() as u8;
}

/// Render the icon at `size` x `size` with a transparent background.
fn render(size: u32) -> RgbaImage {
    // Supersample for smooth edges.
    let ss = 4u32;
    let big = size * ss;
    let mut img = RgbaImage::from_pixel(big, big, Rgba([0, 0, 0, 0]));

    let scale = (big as f32) / 100.0; // design in a 100x100 space
    let panel_cx = 50.0 * scale;
    let panel_cy = 50.0 * scale;
    let panel_hw = 46.0 * scale;
    let panel_hh = 36.0 * scale;
    let panel_r = 10.0 * scale;

    // Grid geometry inside the panel.
    let cols = 5;
    let rows = 3;
    let margin = 8.0 * scale;
    let gap = 3.5 * scale;
    let inner_w = panel_hw * 2.0 - margin * 2.0;
    let inner_h = panel_hh * 2.0 - margin * 2.0;
    let key_w = (inner_w - gap * (cols as f32 - 1.0)) / cols as f32;
    let key_h = (inner_h - gap * (rows as f32 - 1.0)) / rows as f32;
    let key_r = 3.0 * scale;
    let origin_x = panel_cx - panel_hw + margin;
    let origin_y = panel_cy - panel_hh + margin;

    for y in 0..big {
        for x in 0..big {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let mut pixel = Rgba([0, 0, 0, 0]);

            // Panel body (with a subtle lighter edge ring).
            let d = rounded_rect_sdf(px, py, panel_cx, panel_cy, panel_hw, panel_hh, panel_r);
            let edge = 1.5 * scale;
            if d < 0.0 {
                let edge_mix = ((-d) / edge).clamp(0.0, 1.0);
                let body = [
                    (PANEL[0] as f32 * edge_mix + PANEL_EDGE[0] as f32 * (1.0 - edge_mix)) as u8,
                    (PANEL[1] as f32 * edge_mix + PANEL_EDGE[1] as f32 * (1.0 - edge_mix)) as u8,
                    (PANEL[2] as f32 * edge_mix + PANEL_EDGE[2] as f32 * (1.0 - edge_mix)) as u8,
                ];
                let cover = (-d).clamp(0.0, 1.0);
                blend(&mut pixel, body, cover.max(0.85));
            } else {
                let aa = (1.0 - d).clamp(0.0, 1.0);
                if aa > 0.0 {
                    blend(&mut pixel, PANEL_EDGE, aa * 0.9);
                }
            }

            // Keys on top.
            for r in 0..rows {
                for c in 0..cols {
                    let kc = KEY_COLORS[(r * cols + c) as usize];
                    let cx = origin_x + key_w * 0.5 + c as f32 * (key_w + gap);
                    let cy = origin_y + key_h * 0.5 + r as f32 * (key_h + gap);
                    let kd = rounded_rect_sdf(px, py, cx, cy, key_w * 0.5, key_h * 0.5, key_r);
                    if kd < 1.0 {
                        let cover = (1.0 - kd).clamp(0.0, 1.0);
                        // Slight vertical sheen for depth.
                        let sheen =
                            1.0 - 0.18 * ((py - (cy - key_h * 0.5)) / key_h).clamp(0.0, 1.0);
                        let shaded = [
                            (kc[0] as f32 * sheen) as u8,
                            (kc[1] as f32 * sheen) as u8,
                            (kc[2] as f32 * sheen) as u8,
                        ];
                        blend(&mut pixel, shaded, cover);
                    }
                }
            }

            img.put_pixel(x, y, pixel);
        }
    }

    // Downsample to the target size for anti-aliasing.
    image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3)
}

fn main() {
    std::fs::create_dir_all("assets/icons").expect("create assets/icons");
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512] {
        let icon = render(size);
        let path = format!("assets/icons/streamdeck-{size}.png");
        icon.save(&path).expect("save icon");
        println!("wrote {path}");
    }
    // A canonical name used by the tray / desktop entry.
    render(256)
        .save("assets/icons/streamdeck.png")
        .expect("save canonical icon");
    println!("wrote assets/icons/streamdeck.png");
}
