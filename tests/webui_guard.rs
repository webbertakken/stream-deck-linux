//! The editor's server, over a real socket, refuses what a hostile web page
//! could send and still serves its own editor.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use streamdeck::runtime::Control;
use streamdeck::webui::WebUi;
use streamdeck::Model;

const ORIGINAL: &str = "brightness = 40\n";
const PLANTED: &str = r#"{"buttons":[{"key":0,"label":"x","watch":"touch /tmp/pwned"}]}"#;

struct Editor {
    port: u16,
    config: PathBuf,
    _control: Receiver<Control>,
}

fn start_editor(name: &str) -> Editor {
    let dir = std::env::temp_dir().join(format!("sd-guard-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.toml");
    std::fs::write(&config, ORIGINAL).unwrap();
    let (tx, control) = mpsc::channel();
    let web = WebUi::bind("127.0.0.1:0", Model::MK2, config.clone(), tx).unwrap();
    let port: u16 = web.url().rsplit(':').next().unwrap().parse().unwrap();
    let shutdown: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
    let web = Arc::new(web);
    std::thread::spawn(move || web.serve(shutdown));
    Editor {
        port,
        config,
        _control: control,
    }
}

/// Send a raw HTTP/1.1 request and return the status code and body.
fn send(port: u16, head: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let request = format!(
        "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response[9..12].parse().unwrap();
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

#[test]
fn refuses_a_cross_origin_simple_post_and_keeps_the_config() {
    let editor = start_editor("cross-origin");
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
    let editor = start_editor("rebound");
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
    let editor = start_editor("not-json");
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
    let editor = start_editor("own");
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
    let editor = start_editor("page");
    let head = format!("GET / HTTP/1.1\r\nHost: localhost:{}", editor.port);
    let (status, body) = send(editor.port, &head, "");
    assert_eq!(status, 200);
    assert!(
        body.contains("<html") || body.contains("<!doctype"),
        "{body}"
    );
}
