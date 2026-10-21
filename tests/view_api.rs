//! The live view over a real socket: what the daemon drew, as upright PNGs.

mod common;

use base64::Engine;
use common::{send, start_editor};
use streamdeck::config::Config;
use streamdeck::render;
use streamdeck::view::{Frame, PageRef};
use streamdeck::Model;

const LAYOUT: &str = "[[buttons]]\nkey = 7\nlabel = \"Screens\"\n";

fn get_view(port: u16, token: &str) -> (u16, serde_json::Value) {
    let head = format!(
        "GET /api/v1/view HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}"
    );
    let (status, body) = send(port, &head, "");
    (status, serde_json::from_str(&body).unwrap())
}

#[test]
fn answers_not_ready_until_the_daemon_draws_then_the_keys_it_drew() {
    let editor = start_editor("view", LAYOUT);
    editor.tokens.issue("cli", None).unwrap();
    let token = editor.tokens.read("cli").unwrap();

    let (status, body) = get_view(editor.port, &token);
    assert_eq!((status, body["error"].as_str()), (503, Some("not-ready")));

    let config = Config::from_toml_str(LAYOUT).unwrap();
    let screens = render::compose(
        &Model::MK2.image,
        std::path::Path::new("."),
        &config.buttons[0],
    )
    .into_canvas();
    let mut frame = Frame::new(
        &Model::MK2,
        PageRef {
            index: 0,
            name: None,
        },
        1,
    );
    frame.set_brightness(40);
    frame.show_key(7, screens.clone(), false);
    editor.view.publish(frame);

    let (status, body) = get_view(editor.port, &token);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["page"],
        serde_json::json!({ "index": 0, "name": null })
    );
    assert_eq!(
        (body["pages"].as_u64(), body["brightness"].as_u64()),
        (Some(1), Some(40))
    );
    assert_eq!(body["standby"], false);
    assert_eq!(body["model"]["keyCount"], 15);
    let keys = body["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 15);
    let png = base64::engine::general_purpose::STANDARD
        .decode(keys[7]["png"].as_str().unwrap())
        .unwrap();
    let shown = image::load_from_memory(&png).unwrap().to_rgb8();
    assert_eq!(shown, screens, "key 7 is the image drawn, upright");
    assert_eq!(keys[7]["held"], false);
}

#[test]
fn refuses_the_view_to_a_caller_without_a_token() {
    let editor = start_editor("view-no-token", LAYOUT);
    let head = format!(
        "GET /api/v1/view HTTP/1.1\r\nHost: 127.0.0.1:{}",
        editor.port
    );
    let (status, body) = send(editor.port, &head, "");
    assert_eq!(status, 401);
    assert!(body.contains("no-token"), "{body}");
}
