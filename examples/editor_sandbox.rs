//! Serve the web editor over a scratch config, with no deck attached, to try
//! or screenshot the editor without touching the real layout. Run with:
//!   cargo run --example editor_sandbox -- /tmp/sandbox.toml [127.0.0.1:0]
//!
//! Prints the editor's URL, then logs each reload the editor asks for.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use streamdeck::view::LiveView;
use streamdeck::webui::WebUi;
use streamdeck::Model;

fn main() {
    let mut args = std::env::args().skip(1);
    let config = PathBuf::from(args.next().unwrap_or_else(|| "/tmp/sandbox.toml".into()));
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:0".into());
    if !config.exists() {
        std::fs::write(&config, "brightness = 40\n").expect("write the scratch config");
    }

    let (control, requests) = mpsc::channel();
    let web = WebUi::bind(&addr, Model::MK2, config.clone(), control, LiveView::new())
        .expect("bind the editor");
    println!("editor for {} at {}", config.display(), web.url());

    std::thread::spawn(move || {
        for request in requests {
            println!("editor asked the daemon for {request:?}");
        }
    });
    let shutdown = AtomicBool::new(false);
    web.serve(&shutdown).expect("serve the editor");
}
