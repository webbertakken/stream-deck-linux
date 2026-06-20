//! In-process web UI for editing the per-key layout.
//!
//! Serves a small single-page editor. Saving writes the TOML config and asks
//! the running daemon to reload, so edits appear on the device immediately.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde::Deserialize;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::model::Model;
use crate::render;
use crate::runtime::Control;

const INDEX_HTML: &str = include_str!("../assets/web/index.html");
const APP_JS: &str = include_str!("../assets/web/app.js");
const APP_CSS: &str = include_str!("../assets/web/app.css");

/// The editor's HTTP server, bound but not yet serving.
pub struct WebUi {
    server: Server,
    model: Model,
    config_path: PathBuf,
    base_dir: PathBuf,
    control: Sender<Control>,
    addr: String,
}

#[derive(Deserialize)]
struct BrightnessBody {
    value: u8,
}

impl WebUi {
    /// Bind the editor to `addr` (e.g. `127.0.0.1:0` for an ephemeral port).
    pub fn bind(
        addr: &str,
        model: Model,
        config_path: PathBuf,
        control: Sender<Control>,
    ) -> Result<Self> {
        let server = Server::http(addr).map_err(|err| Error::Web(err.to_string()))?;
        let resolved = server
            .server_addr()
            .to_ip()
            .map(|a| a.to_string())
            .unwrap_or_else(|| addr.to_string());
        let base_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            server,
            model,
            config_path,
            base_dir,
            control,
            addr: resolved,
        })
    }

    /// The URL the editor is reachable at.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Serve requests until `shutdown` is set.
    pub fn serve(&self, shutdown: &AtomicBool) -> Result<()> {
        while !shutdown.load(Ordering::Relaxed) {
            match self.server.recv_timeout(Duration::from_millis(200)) {
                Ok(Some(request)) => {
                    if let Err(err) = self.route(request) {
                        eprintln!("web ui: {err}");
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    eprintln!("web ui: recv error: {err}");
                    break;
                }
            }
        }
        Ok(())
    }

    fn route(&self, request: Request) -> Result<()> {
        let method = request.method().clone();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("/");

        match (&method, path) {
            (Method::Get, "/") => respond(
                request,
                200,
                "text/html; charset=utf-8",
                INDEX_HTML.as_bytes(),
            ),
            (Method::Get, "/app.js") => respond(
                request,
                200,
                "application/javascript; charset=utf-8",
                APP_JS.as_bytes(),
            ),
            (Method::Get, "/app.css") => {
                respond(request, 200, "text/css; charset=utf-8", APP_CSS.as_bytes())
            }
            (Method::Get, "/api/state") => self.get_state(request),
            (Method::Post, "/api/state") => self.post_state(request),
            (Method::Post, "/api/brightness") => self.post_brightness(request),
            (Method::Get, p) if p.starts_with("/api/preview/") => self.get_preview(request, p),
            _ => respond(request, 404, "text/plain", b"not found"),
        }
    }

    fn load_config(&self) -> Config {
        if self.config_path.exists() {
            Config::load(&self.config_path).unwrap_or_else(|err| {
                eprintln!("web ui: config load failed: {err}");
                Config {
                    brightness: None,
                    buttons: Vec::new(),
                }
            })
        } else {
            Config {
                brightness: None,
                buttons: Vec::new(),
            }
        }
    }

    fn get_state(&self, request: Request) -> Result<()> {
        let config = self.load_config();
        let state = serde_json::json!({
            "model": {
                "name": self.model.name,
                "columns": self.model.columns,
                "rows": self.model.rows,
                "keyCount": self.model.key_count,
            },
            "brightness": config.brightness,
            "buttons": config.buttons,
        });
        let body = serde_json::to_vec(&state).map_err(|e| Error::Web(e.to_string()))?;
        respond(request, 200, "application/json", &body)
    }

    fn post_state(&self, mut request: Request) -> Result<()> {
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .map_err(|e| Error::Web(e.to_string()))?;

        let config: Config = match serde_json::from_str(&body) {
            Ok(config) => config,
            Err(err) => return respond(request, 400, "text/plain", err.to_string().as_bytes()),
        };
        if let Err(err) = config.validate(&self.model) {
            return respond(request, 400, "text/plain", err.to_string().as_bytes());
        }

        let toml = match config.to_toml_string() {
            Ok(toml) => toml,
            Err(err) => return respond(request, 400, "text/plain", err.to_string().as_bytes()),
        };
        if let Err(err) = std::fs::write(&self.config_path, toml) {
            return respond(request, 500, "text/plain", err.to_string().as_bytes());
        }

        let _ = self.control.send(Control::Reload);
        respond(request, 200, "application/json", br#"{"ok":true}"#)
    }

    fn post_brightness(&self, mut request: Request) -> Result<()> {
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .map_err(|e| Error::Web(e.to_string()))?;
        let parsed: BrightnessBody = match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(err) => return respond(request, 400, "text/plain", err.to_string().as_bytes()),
        };
        let _ = self.control.send(Control::SetBrightness(parsed.value));
        respond(request, 200, "application/json", br#"{"ok":true}"#)
    }

    fn get_preview(&self, request: Request, path: &str) -> Result<()> {
        let key: u8 = match path
            .trim_start_matches("/api/preview/")
            .trim_end_matches(".png")
            .parse()
        {
            Ok(key) => key,
            Err(_) => return respond(request, 400, "text/plain", b"bad key"),
        };

        let config = self.load_config();
        let button = config.buttons.iter().find(|b| b.key == key).cloned();
        let button = button.unwrap_or(crate::config::ButtonConfig {
            key,
            color: Some("#000000".into()),
            ..Default::default()
        });
        let png = render::button_png(&self.model.image, &self.base_dir, &button)?;
        respond(request, 200, "image/png", &png)
    }
}

fn respond(request: Request, status: u16, content_type: &str, body: &[u8]) -> Result<()> {
    let header = Header::from_bytes(b"Content-Type".as_slice(), content_type.as_bytes())
        .map_err(|_| Error::Web("invalid content-type".into()))?;
    let response = Response::from_data(body)
        .with_status_code(status)
        .with_header(header);
    request
        .respond(response)
        .map_err(|e| Error::Web(e.to_string()))
}
