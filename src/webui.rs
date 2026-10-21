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
use crate::control::{self, ApiRequest, ControlApi};
use crate::error::{Error, Result};
use crate::guard::{self, RequestFacts};
use crate::model::Model;
use crate::render;
use crate::runtime::Control;
use crate::tokens::{self, TokenStore};
use crate::view::LiveView;

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
    port: u16,
    api: ControlApi,
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
        view: LiveView,
    ) -> Result<Self> {
        let server = Server::http(addr).map_err(|err| Error::Web(err.to_string()))?;
        let ip = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| Error::Web(format!("{addr} is not an IP address")))?;
        let base_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let api = ControlApi {
            config_path: config_path.clone(),
            model,
            tokens: TokenStore::new(tokens::default_dir()),
            control: control.clone(),
            view,
        };
        Ok(Self {
            server,
            model,
            config_path,
            base_dir,
            control,
            api,
            addr: ip.to_string(),
            port: ip.port(),
        })
    }

    /// Read caller tokens from `store` instead of the default directory.
    pub fn with_tokens(mut self, store: TokenStore) -> Self {
        self.api.tokens = store;
        self
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

        let header = |name: &'static str| {
            request
                .headers()
                .iter()
                .find(|h| h.field.equiv(name))
                .map(|h| h.value.as_str().to_string())
        };
        let (host, origin, content_type) =
            (header("Host"), header("Origin"), header("Content-Type"));
        let facts = RequestFacts {
            is_write: !matches!(method, Method::Get | Method::Head),
            host: host.as_deref(),
            origin: origin.as_deref(),
            content_type: content_type.as_deref(),
        };
        if let Err(refusal) = guard::admit(&facts, self.port) {
            eprintln!(
                "web ui: refused {method} {path}: {} (host={host:?} origin={origin:?})",
                refusal.name()
            );
            return respond(
                request,
                refusal.status(),
                "text/plain",
                refusal.name().as_bytes(),
            );
        }

        if path.starts_with(control::PREFIX) {
            let authorization = header("Authorization");
            let if_match = header("If-Match");
            return self.serve_control(request, path.to_string(), authorization, if_match);
        }

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
            (Method::Get, "/api/apps") => self.get_apps(request),
            (Method::Get, "/api/icon") => self.get_icon(request, &url),
            (Method::Get, "/api/state") => self.get_state(request),
            (Method::Post, "/api/state") => self.post_state(request),
            (Method::Post, "/api/brightness") => self.post_brightness(request),
            (Method::Get, p) if p.starts_with("/api/preview/") => self.get_preview(request, p),
            _ => respond(request, 404, "text/plain", b"not found"),
        }
    }

    fn serve_control(
        &self,
        mut request: Request,
        path: String,
        authorization: Option<String>,
        if_match: Option<String>,
    ) -> Result<()> {
        let mut body = String::new();
        if let Err(err) = request.as_reader().read_to_string(&mut body) {
            let refusal = serde_json::json!({ "error": "bad-json", "detail": err.to_string() });
            return respond(
                request,
                400,
                "application/json",
                refusal.to_string().as_bytes(),
            );
        }
        let answer = self.api.handle(&ApiRequest {
            method: request.method().as_str().to_uppercase(),
            path,
            authorization,
            if_match,
            body,
        });
        let bytes = serde_json::to_vec(&answer.body).map_err(|e| Error::Web(e.to_string()))?;
        respond(request, answer.status, "application/json", &bytes)
    }

    fn load_config(&self) -> Config {
        let empty = Config {
            brightness: None,
            press_feedback: None,
            buttons: Vec::new(),
            pages: Vec::new(),
        };
        if self.config_path.exists() {
            Config::load(&self.config_path).unwrap_or_else(|err| {
                eprintln!("web ui: config load failed: {err}");
                empty
            })
        } else {
            empty
        }
    }

    fn get_apps(&self, request: Request) -> Result<()> {
        let apps = crate::apps::list();
        let body = serde_json::to_vec(&apps).map_err(|e| Error::Web(e.to_string()))?;
        respond(request, 200, "application/json", &body)
    }

    /// Serve an application icon PNG by absolute path. Restricted to existing
    /// `.png` files (the app list only ever yields resolved PNG icon paths).
    fn get_icon(&self, request: Request, url: &str) -> Result<()> {
        let path = url.split_once("path=").map(|(_, p)| percent_decode(p));
        let Some(path) = path else {
            return respond(request, 400, "text/plain", b"missing path");
        };
        let lower = path.to_ascii_lowercase();
        if !lower.ends_with(".png") {
            return respond(request, 400, "text/plain", b"only png icons");
        }
        match std::fs::read(&path) {
            Ok(bytes) => respond(request, 200, "image/png", &bytes),
            Err(_) => respond(request, 404, "text/plain", b"not found"),
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
            "press_feedback": config.press_feedback,
            "pages": config.pages(),
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

    /// `GET /api/preview/<page>/<key>.png` (or `/api/preview/<key>.png` for
    /// page 0): the rendered key image for that page.
    fn get_preview(&self, request: Request, path: &str) -> Result<()> {
        let rest = path
            .trim_start_matches("/api/preview/")
            .trim_end_matches(".png");
        let mut parts = rest.split('/');
        let first = parts.next();
        let second = parts.next();
        let (page, key) = match (first, second) {
            (Some(a), Some(b)) => (a.parse::<usize>().ok(), b.parse::<u8>().ok()),
            (Some(a), None) => (Some(0usize), a.parse::<u8>().ok()),
            _ => (None, None),
        };
        let (Some(page), Some(key)) = (page, key) else {
            return respond(request, 400, "text/plain", b"bad preview path");
        };

        let config = self.load_config();
        let pages = config.pages();
        let button = pages
            .get(page)
            .and_then(|p| p.buttons.iter().find(|b| b.key == key).cloned())
            .unwrap_or(crate::config::ButtonConfig {
                key,
                color: Some("#000000".into()),
                ..Default::default()
            });
        let png = render::button_png(&self.model.image, &self.base_dir, &button)?;
        respond(request, 200, "image/png", &png)
    }
}

/// Minimal percent-decoder for a single query-string value.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
