//! Time one `GET /api/v1/view` answer: 15 composed keys encoded as base64 PNGs.
//! Run on a release build:
//!   cargo run --release --example view_timing

use std::path::Path;
use std::time::{Duration, Instant};

use streamdeck::config::ButtonConfig;
use streamdeck::model::Model;
use streamdeck::render;
use streamdeck::view::{Frame, PageRef};

const ROUNDS: usize = 200;

fn main() {
    let model = Model::MK2;
    let mut frame = Frame::new(
        &model,
        PageRef {
            index: 0,
            name: None,
        },
        1,
    );
    for key in 0..model.key_count {
        let button = ButtonConfig {
            key,
            color: Some(format!("#{:02x}3355", key * 16)),
            label: Some(format!("Key {key}")),
            ..Default::default()
        };
        let surface = render::compose(&model.image, Path::new("."), &button);
        frame.show_key(key, surface.into_canvas(), false);
    }

    let mut times: Vec<Duration> = (0..ROUNDS)
        .map(|_| {
            let started = Instant::now();
            let body = frame.to_json().expect("the frame encodes");
            std::hint::black_box(body);
            started.elapsed()
        })
        .collect();
    times.sort();

    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let mean = times.iter().sum::<Duration>() / ROUNDS as u32;
    let bytes = frame
        .to_json()
        .expect("the frame encodes")
        .to_string()
        .len();
    println!(
        "{ROUNDS} answers of {} keys ({bytes} bytes of JSON): mean {:.2} ms, median {:.2} ms, p99 {:.2} ms, max {:.2} ms",
        model.key_count,
        ms(mean),
        ms(times[ROUNDS / 2]),
        ms(times[ROUNDS * 99 / 100]),
        ms(times[ROUNDS - 1]),
    );
}
