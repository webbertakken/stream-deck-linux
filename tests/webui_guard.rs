//! The editor's server, over a real socket, refuses what a hostile web page
//! could send and still serves its own editor.

mod common;

use common::{send, start_editor};

const ORIGINAL: &str = "brightness = 40\n";
const PLANTED: &str = r#"{"buttons":[{"key":0,"label":"x","watch":"touch /tmp/pwned"}]}"#;

#[test]
fn refuses_a_cross_origin_simple_post_and_keeps_the_config() {
    let editor = start_editor("cross-origin", ORIGINAL);
    let head = format!(
        "POST /api/state HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nOrigin: https://evil.example\r\nContent-Type: text/plain",
        editor.port
    );
    let (status, body) = send(editor.port, &head, PLANTED);
    assert_eq!((status, body.as_str()), (403, "foreign-origin"));
    assert_eq!(std::fs::read_to_string(&editor.config).unwrap(), ORIGINAL);
}

#[test]
fn refuses_a_rebound_host() {
    let editor = start_editor("rebound", ORIGINAL);
    let head = format!(
        "POST /api/state HTTP/1.1\r\nHost: evil.example:{}\r\nContent-Type: application/json",
        editor.port
    );
    let (status, body) = send(editor.port, &head, PLANTED);
    assert_eq!((status, body.as_str()), (403, "foreign-host"));
    assert_eq!(std::fs::read_to_string(&editor.config).unwrap(), ORIGINAL);
}

#[test]
fn refuses_a_write_that_is_not_json() {
    let editor = start_editor("not-json", ORIGINAL);
    let head = format!(
        "POST /api/state HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: text/plain",
        editor.port
    );
    let (status, body) = send(editor.port, &head, PLANTED);
    assert_eq!((status, body.as_str()), (415, "not-json"));
    assert_eq!(std::fs::read_to_string(&editor.config).unwrap(), ORIGINAL);
}

#[test]
fn accepts_its_own_editor_writing_json() {
    let editor = start_editor("own", ORIGINAL);
    let head = format!(
        "POST /api/state HTTP/1.1\r\nHost: 127.0.0.1:{0}\r\nOrigin: http://127.0.0.1:{0}\r\nContent-Type: application/json",
        editor.port
    );
    let (status, _) = send(editor.port, &head, r#"{"brightness":55,"buttons":[]}"#);
    assert_eq!(status, 200);
    let written = std::fs::read_to_string(&editor.config).unwrap();
    assert!(written.contains("brightness = 55"), "{written}");
}

#[test]
fn serves_the_editor_page_by_its_loopback_name() {
    let editor = start_editor("page", ORIGINAL);
    let head = format!("GET / HTTP/1.1\r\nHost: localhost:{}", editor.port);
    let (status, body) = send(editor.port, &head, "");
    assert_eq!(status, 200);
    assert!(
        body.contains("<html") || body.contains("<!doctype"),
        "{body}"
    );
}
