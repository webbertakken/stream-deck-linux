//! Icons and the style over a real socket: open reads anyone on loopback may
//! make, still behind the guard; a key takes an icon only by a known name.

mod common;

use common::{send, start_editor};
use serde_json::Value;

const ORIGINAL: &str = "brightness = 40\n";

fn get(port: u16, target: &str) -> (u16, String) {
    send(
        port,
        &format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}"),
        "",
    )
}

#[test]
fn searches_icons_without_a_token() {
    let editor = start_editor("icons", ORIGINAL);
    let (status, body) = get(editor.port, "/api/v1/icons?q=mail");
    assert_eq!(status, 200, "{body}");
    let found: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(found["icons"][0]["name"], "mail");
    assert!(found["total"].as_u64().unwrap() >= found["icons"].as_array().unwrap().len() as u64);
    assert!(found["icons"][0]["tags"]
        .as_array()
        .unwrap()
        .contains(&"email".into()));
}

#[test]
fn answers_the_style_without_a_token() {
    let editor = start_editor("style", ORIGINAL);
    let (status, body) = get(editor.port, "/api/v1/style");
    assert_eq!(status, 200, "{body}");
    let style: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(style["palette"][0]["name"], "apps");
    assert_eq!(style["palette"][0]["hex"], "#2E5A88");
    assert!(style["layout"].as_array().is_some_and(|r| !r.is_empty()));
}

#[test]
fn the_guard_still_stands_before_the_open_reads() {
    let editor = start_editor("icons-guarded", ORIGINAL);
    let head = format!(
        "GET /api/v1/icons?q=mail HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nOrigin: https://evil.example",
        editor.port
    );
    assert_eq!(send(editor.port, &head, ""), (403, "foreign-origin".into()));
    let rebound = format!(
        "GET /api/v1/style HTTP/1.1\r\nHost: evil.example:{}",
        editor.port
    );
    assert_eq!(
        send(editor.port, &rebound, ""),
        (403, "foreign-host".into())
    );
}

#[test]
fn a_key_takes_a_known_icon_and_refuses_a_typo_by_name() {
    let editor = start_editor("icon-key", ORIGINAL);
    editor.tokens.issue("runa", None).unwrap();
    let token = editor.tokens.read("runa").unwrap();
    let put = format!(
        "PUT /api/v1/pages/0/keys/2 HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json",
        editor.port
    );

    let (status, body) = send(editor.port, &put, r#"{"icon":"mial"}"#);
    assert_eq!(status, 422, "{body}");
    let refusal: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(refusal["error"], "invalid");
    assert_eq!(refusal["detail"], "unknown icon 'mial'; did you mean mail?");
    assert_eq!(std::fs::read_to_string(&editor.config).unwrap(), ORIGINAL);

    let (status, body) = send(
        editor.port,
        &put,
        r##"{"icon":"mail","label":"Mail","color":"#2E5A88"}"##,
    );
    assert_eq!(status, 200, "{body}");
    assert!(std::fs::read_to_string(&editor.config)
        .unwrap()
        .contains("icon = \"mail\""));
}

#[test]
fn the_editor_serves_an_icon_svg_by_name_only() {
    let editor = start_editor("icon-svg", ORIGINAL);
    let (status, body) = get(editor.port, "/api/lucide/mail.svg");
    assert_eq!(status, 200);
    assert!(
        body.contains("<svg") && body.contains("lucide-mail"),
        "{body}"
    );
    for missing in [
        "/api/lucide/mial.svg",
        "/api/lucide/mail",
        "/api/lucide/..%2Fconfig.svg",
    ] {
        assert_eq!(get(editor.port, missing).0, 404, "{missing}");
    }
}
