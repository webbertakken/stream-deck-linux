//! The control API over a real socket: a caller with a token changes a key;
//! the guard still stands in front of it.

mod common;

use common::{send, start_editor};

const ORIGINAL: &str = "brightness = 40\n";

#[test]
fn a_caller_with_a_token_sets_a_key_and_reads_it_back() {
    let editor = start_editor("set-key", ORIGINAL);
    editor.tokens.issue("runa", None).unwrap();
    let token = editor.tokens.read("runa").unwrap();
    let auth = format!(
        "Host: 127.0.0.1:{}\r\nAuthorization: Bearer {token}",
        editor.port
    );

    let put =
        format!("PUT /api/v1/pages/0/keys/2 HTTP/1.1\r\n{auth}\r\nContent-Type: application/json");
    let (status, body) = send(editor.port, &put, r##"{"label":"Mail","color":"#223344"}"##);
    assert_eq!(status, 200, "{body}");

    let (status, body) = send(
        editor.port,
        &format!("GET /api/v1/layout HTTP/1.1\r\n{auth}"),
        "",
    );
    assert_eq!(status, 200);
    let layout: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(layout["pages"][0]["buttons"][0]["label"], "Mail");
    assert!(std::fs::read_to_string(&editor.config)
        .unwrap()
        .contains("Mail"));
}

#[test]
fn refuses_a_caller_without_a_token() {
    let editor = start_editor("no-token", ORIGINAL);
    let head = format!(
        "GET /api/v1/layout HTTP/1.1\r\nHost: 127.0.0.1:{}",
        editor.port
    );
    let (status, body) = send(editor.port, &head, "");
    assert_eq!(status, 401);
    assert!(body.contains("no-token"), "{body}");
}

#[test]
fn the_guard_refuses_a_web_page_even_with_a_token() {
    let editor = start_editor("guarded", ORIGINAL);
    editor.tokens.issue("runa", None).unwrap();
    let token = editor.tokens.read("runa").unwrap();
    let head = format!(
        "PUT /api/v1/brightness HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nOrigin: https://evil.example\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json",
        editor.port
    );
    let (status, _) = send(editor.port, &head, r#"{"value":10}"#);
    assert_eq!(status, 403);
    assert_eq!(std::fs::read_to_string(&editor.config).unwrap(), ORIGINAL);
}
