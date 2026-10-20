//! A real editor server on an ephemeral loopback port, and a raw HTTP client.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use streamdeck::runtime::Control;
use streamdeck::tokens::TokenStore;
use streamdeck::webui::WebUi;
use streamdeck::Model;

pub struct Editor {
    pub port: u16,
    pub config: PathBuf,
    pub tokens: TokenStore,
    _control: Receiver<Control>,
}

pub fn start_editor(name: &str, config_text: &str) -> Editor {
    let dir = std::env::temp_dir().join(format!("sd-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.toml");
    std::fs::write(&config, config_text).unwrap();
    let tokens = TokenStore::new(dir.join("tokens"));
    let (tx, control) = mpsc::channel();
    let web = WebUi::bind("127.0.0.1:0", Model::MK2, config.clone(), tx)
        .unwrap()
        .with_tokens(tokens.clone());
    let port: u16 = web.url().rsplit(':').next().unwrap().parse().unwrap();
    let shutdown: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
    let web = Arc::new(web);
    std::thread::spawn(move || web.serve(shutdown));
    Editor {
        port,
        config,
        tokens,
        _control: control,
    }
}

/// Send a raw HTTP/1.1 request (`head` without the blank line); answer status and body.
pub fn send(port: u16, head: &str, body: &str) -> (u16, String) {
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
