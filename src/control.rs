//! The control API, `/api/v1` (`docs/control-api.md`).
//!
//! Kept apart from HTTP: [`ControlApi::handle`] takes a parsed request and
//! answers a status and a JSON body, so every rule is tested without a
//! socket. The web server routes `/api/v1/*` here.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::{ButtonConfig, Config};
use crate::layout::{self, Edit, EditError};
use crate::model::Model;
use crate::runtime::Control;
use crate::schema;
use crate::tokens::TokenStore;
use crate::view::LiveView;

/// The prefix every control route shares.
pub const PREFIX: &str = "/api/v1/";

/// A control route, parsed from a method and a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Layout,
    Schema,
    SetKey { page: String, key: u8 },
    ClearKey { page: String, key: u8 },
    AddPage,
    RemovePage { page: String },
    Brightness,
}

/// Parse `method path` into a route; `None` is `not-found`.
pub fn parse_route(method: &str, path: &str) -> Option<Route> {
    let rest = path.strip_prefix(PREFIX)?;
    let parts: Vec<String> = rest.split('/').map(percent_decode).collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    match (method, parts.as_slice()) {
        ("GET", ["layout"]) => Some(Route::Layout),
        ("GET", ["schema"]) => Some(Route::Schema),
        ("PUT", ["pages", page, "keys", key]) if !page.is_empty() => Some(Route::SetKey {
            page: page.to_string(),
            key: key.parse().ok()?,
        }),
        ("DELETE", ["pages", page, "keys", key]) if !page.is_empty() => Some(Route::ClearKey {
            page: page.to_string(),
            key: key.parse().ok()?,
        }),
        ("POST", ["pages"]) => Some(Route::AddPage),
        ("DELETE", ["pages", page]) if !page.is_empty() => Some(Route::RemovePage {
            page: page.to_string(),
        }),
        ("PUT", ["brightness"]) => Some(Route::Brightness),
        _ => None,
    }
}

/// A request, as the web server hands it on.
#[derive(Debug, Clone, Default)]
pub struct ApiRequest {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub if_match: Option<String>,
    pub body: String,
}

/// An answer: a status and a JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiResponse {
    pub status: u16,
    pub body: Value,
}

fn refusal(status: u16, name: &str, detail: impl Into<String>) -> ApiResponse {
    ApiResponse {
        status,
        body: json!({ "error": name, "detail": detail.into() }),
    }
}

fn ok(body: Value) -> ApiResponse {
    ApiResponse { status: 200, body }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageBody {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrightnessBody {
    value: u8,
}

/// The control API over one config file.
pub struct ControlApi {
    pub config_path: PathBuf,
    pub model: Model,
    pub tokens: TokenStore,
    pub control: Sender<Control>,
    /// What the device shows, as the daemon last drew it.
    pub view: LiveView,
}

impl ControlApi {
    /// Answer one request; logs who asked and how it ended.
    pub fn handle(&self, request: &ApiRequest) -> ApiResponse {
        let (caller, response) = self.decide(request);
        let outcome = response.body["error"].as_str().unwrap_or("ok");
        eprintln!(
            "control: {} {} {} -> {} {outcome}",
            caller.as_deref().unwrap_or("-"),
            request.method,
            request.path,
            response.status
        );
        response
    }

    fn decide(&self, request: &ApiRequest) -> (Option<String>, ApiResponse) {
        let Some(token) = request
            .authorization
            .as_deref()
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return (
                None,
                refusal(401, "no-token", "send Authorization: Bearer <token>"),
            );
        };
        let Some(caller) = self.tokens.caller_of(token) else {
            return (
                None,
                refusal(401, "bad-token", "no caller holds this token"),
            );
        };
        let Some(route) = parse_route(&request.method, &request.path) else {
            let detail = format!("no route {} {}", request.method, request.path);
            return (Some(caller), refusal(404, "not-found", detail));
        };
        (Some(caller), self.answer(route, request))
    }

    fn answer(&self, route: Route, request: &ApiRequest) -> ApiResponse {
        let (config, bytes) = match read_config(&self.config_path) {
            Ok(read) => read,
            Err(detail) => return refusal(500, "unreadable", detail),
        };
        let revision = layout::revision(&bytes);

        let edit = match route {
            Route::Layout => return ok(layout_body(&config, &self.model, &revision)),
            Route::Schema => return ok(schema::schema()),
            Route::SetKey { page, key } => match parse_button(&request.body, key) {
                Ok(button) => Edit::SetKey {
                    page,
                    key,
                    button: Box::new(button),
                },
                Err(detail) => return refusal(400, "bad-json", detail),
            },
            Route::ClearKey { page, key } => Edit::ClearKey { page, key },
            Route::AddPage => match serde_json::from_str::<PageBody>(&request.body) {
                Ok(body) => Edit::AddPage { name: body.name },
                Err(err) => return refusal(400, "bad-json", err.to_string()),
            },
            Route::RemovePage { page } => Edit::RemovePage { page },
            Route::Brightness => match serde_json::from_str::<BrightnessBody>(&request.body) {
                Ok(body) => Edit::Brightness(body.value),
                Err(err) => return refusal(400, "bad-json", err.to_string()),
            },
        };

        if let Some(expected) = &request.if_match {
            let expected = expected.trim().trim_matches('"');
            if expected != revision {
                let detail = format!("the layout is at {revision}, not {expected}; read it again");
                return refusal(409, "stale-revision", detail);
            }
        }

        let applied = match layout::apply(&config, edit, &self.model) {
            Ok(applied) => applied,
            Err(err) => return edit_refusal(&err),
        };
        let written = match write_config(&self.config_path, &applied.config) {
            Ok(bytes) => bytes,
            Err(detail) => return refusal(500, "write-failed", detail),
        };
        let _ = self.control.send(Control::Reload);

        let mut body = json!({ "revision": layout::revision(&written) });
        if let Some(page) = applied.page {
            body["page"] = json!(page);
        }
        ok(body)
    }
}

fn edit_refusal(err: &EditError) -> ApiResponse {
    let status = match err {
        EditError::NoSuchPage(_) => 404,
        EditError::LastPage => 409,
        EditError::Invalid(_) => 422,
    };
    refusal(status, err.name(), err.detail())
}

/// A button from a body; a `key` in it must be the route's.
fn parse_button(body: &str, key: u8) -> Result<ButtonConfig, String> {
    let mut value: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "a button is a JSON object".to_string())?;
    match object.get("key") {
        None => {
            object.insert("key".into(), json!(key));
        }
        Some(given) if given == &json!(key) => {}
        Some(given) => return Err(format!("the body's key {given} is not the route's {key}")),
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn layout_body(config: &Config, model: &Model, revision: &str) -> Value {
    let pages: Vec<Value> = config
        .pages()
        .into_iter()
        .enumerate()
        .map(|(index, page)| json!({ "index": index, "name": page.name, "buttons": page.buttons }))
        .collect();
    json!({
        "revision": revision,
        "model": {
            "name": model.name,
            "columns": model.columns,
            "rows": model.rows,
            "keyCount": model.key_count,
        },
        "brightness": config.brightness,
        "press_feedback": config.press_feedback,
        "pages": pages,
    })
}

/// The config and its bytes; an absent file is an empty layout.
fn read_config(path: &Path) -> Result<(Config, Vec<u8>), String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(format!("{}: {err}", path.display())),
    };
    let text = String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?;
    let config = Config::from_toml_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((config, bytes))
}

/// Write atomically (temp file, then rename), answering the bytes written.
fn write_config(path: &Path, config: &Config) -> Result<Vec<u8>, String> {
    let toml = config.to_toml_string().map_err(|e| e.to_string())?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, &toml).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(toml.into_bytes())
}

/// Decode `%XX` escapes in one path segment.
fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

    struct Fixture {
        api: ControlApi,
        token: String,
        control: Receiver<Control>,
    }

    fn fixture(name: &str, config: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("sd-control-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        std::fs::write(&config_path, config).unwrap();
        let tokens = TokenStore::new(dir.join("tokens"));
        tokens.issue("runa", None).unwrap();
        let token = tokens.read("runa").unwrap();
        let (tx, control) = mpsc::channel();
        Fixture {
            api: ControlApi {
                config_path,
                model: Model::MK2,
                tokens,
                control: tx,
                view: LiveView::new(),
            },
            token,
            control,
        }
    }

    impl Fixture {
        fn call(&self, method: &str, path: &str, body: &str) -> ApiResponse {
            self.call_if(method, path, body, None)
        }

        fn call_if(
            &self,
            method: &str,
            path: &str,
            body: &str,
            if_match: Option<&str>,
        ) -> ApiResponse {
            self.api.handle(&ApiRequest {
                method: method.into(),
                path: path.into(),
                authorization: Some(format!("Bearer {}", self.token)),
                if_match: if_match.map(String::from),
                body: body.into(),
            })
        }

        fn file(&self) -> String {
            std::fs::read_to_string(&self.api.config_path).unwrap()
        }
    }

    const START: &str = "brightness = 40\n\n[[buttons]]\nkey = 0\nlabel = \"Home\"\n";

    #[test]
    fn parses_each_route_and_nothing_else() {
        let set = Route::SetKey {
            page: "My page".into(),
            key: 3,
        };
        assert_eq!(parse_route("GET", "/api/v1/layout"), Some(Route::Layout));
        assert_eq!(parse_route("GET", "/api/v1/schema"), Some(Route::Schema));
        assert_eq!(
            parse_route("PUT", "/api/v1/pages/My%20page/keys/3"),
            Some(set)
        );
        assert_eq!(
            parse_route("DELETE", "/api/v1/pages/0/keys/3"),
            Some(Route::ClearKey {
                page: "0".into(),
                key: 3
            })
        );
        assert_eq!(parse_route("POST", "/api/v1/pages"), Some(Route::AddPage));
        assert_eq!(
            parse_route("DELETE", "/api/v1/pages/Media"),
            Some(Route::RemovePage {
                page: "Media".into()
            })
        );
        assert_eq!(
            parse_route("PUT", "/api/v1/brightness"),
            Some(Route::Brightness)
        );
        for (method, path) in [
            ("GET", "/api/v1/nope"),
            ("POST", "/api/v1/layout"),
            ("PUT", "/api/v1/pages/0/keys/x"),
            ("PUT", "/api/v1/pages/0/keys/300"),
            ("DELETE", "/api/v1/pages/"),
            ("GET", "/api/state"),
        ] {
            assert_eq!(parse_route(method, path), None, "{method} {path}");
        }
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn refuses_a_missing_or_unknown_token_before_anything_else() {
        let fx = fixture("auth", START);
        let mut request = ApiRequest {
            method: "GET".into(),
            path: "/api/v1/layout".into(),
            ..Default::default()
        };
        assert_eq!(fx.api.handle(&request).body["error"], "no-token");
        request.authorization = Some("Bearer sdk_runa_0000".into());
        let response = fx.api.handle(&request);
        assert_eq!(
            (response.status, response.body["error"].as_str()),
            (401, Some("bad-token"))
        );
        request.authorization = Some(format!("Bearer {}", fx.token));
        request.path = "/api/v1/nope".into();
        assert_eq!(fx.api.handle(&request).body["error"], "not-found");
    }

    #[test]
    fn answers_the_layout_with_its_revision_and_normalised_pages() {
        let fx = fixture("layout", START);
        let response = fx.call("GET", "/api/v1/layout", "");
        assert_eq!(response.status, 200);
        assert_eq!(
            response.body["revision"],
            layout::revision(START.as_bytes())
        );
        assert_eq!(response.body["model"]["keyCount"], 15);
        assert_eq!(response.body["brightness"], 40);
        assert_eq!(response.body["pages"][0]["index"], 0);
        assert_eq!(response.body["pages"][0]["buttons"][0]["label"], "Home");
        assert_eq!(fx.call("GET", "/api/v1/schema", "").body["api"], 1);
    }

    #[test]
    fn sets_a_key_writes_the_file_reloads_and_answers_the_new_revision() {
        let fx = fixture("set", START);
        let response = fx.call(
            "PUT",
            "/api/v1/pages/0/keys/4",
            r##"{"label":"Mail","color":"#223344","run":"thunderbird"}"##,
        );
        assert_eq!(response.status, 200, "{}", response.body);
        let file = fx.file();
        assert!(
            file.contains("key = 4") && file.contains("thunderbird"),
            "{file}"
        );
        assert_eq!(response.body["revision"], layout::revision(file.as_bytes()));
        assert_eq!(fx.control.try_recv().unwrap(), Control::Reload);
    }

    #[test]
    fn adds_a_page_answering_its_index_and_removes_it() {
        let fx = fixture("pages", START);
        let added = fx.call("POST", "/api/v1/pages", r#"{"name":"Media"}"#);
        assert_eq!((added.status, added.body["page"].as_u64()), (200, Some(1)));
        let removed = fx.call("DELETE", "/api/v1/pages/Media", "");
        assert_eq!(removed.status, 200);
        assert!(!fx.file().contains("Media"));
    }

    #[test]
    fn stores_brightness_and_clears_a_key() {
        let fx = fixture("bright", START);
        assert_eq!(
            fx.call("PUT", "/api/v1/brightness", r#"{"value":70}"#)
                .status,
            200
        );
        assert_eq!(fx.call("DELETE", "/api/v1/pages/0/keys/0", "").status, 200);
        let file = fx.file();
        assert!(
            file.contains("brightness = 70") && !file.contains("Home"),
            "{file}"
        );
    }

    #[test]
    fn writes_only_over_the_revision_it_was_given() {
        let fx = fixture("revision", START);
        let current = layout::revision(START.as_bytes());
        let stale = fx.call_if(
            "PUT",
            "/api/v1/brightness",
            r#"{"value":70}"#,
            Some("0000000000000000"),
        );
        assert_eq!(
            (stale.status, stale.body["error"].as_str()),
            (409, Some("stale-revision"))
        );
        assert_eq!(fx.file(), START);
        let quoted = format!("\"{current}\"");
        let fresh = fx.call_if(
            "PUT",
            "/api/v1/brightness",
            r#"{"value":70}"#,
            Some(&quoted),
        );
        assert_eq!(fresh.status, 200);
    }

    #[test]
    fn refuses_each_bad_change_by_name_and_leaves_the_file() {
        let fx = fixture("refuse", START);
        let cases = [
            ("PUT", "/api/v1/pages/0/keys/4", "not json", 400, "bad-json"),
            ("PUT", "/api/v1/pages/0/keys/4", "[1]", 400, "bad-json"),
            (
                "PUT",
                "/api/v1/pages/0/keys/4",
                r#"{"key":5,"label":"x"}"#,
                400,
                "bad-json",
            ),
            (
                "PUT",
                "/api/v1/pages/0/keys/4",
                r#"{"lable":"x"}"#,
                400,
                "bad-json",
            ),
            (
                "PUT",
                "/api/v1/pages/0/keys/4",
                r#"{"run":"x"}"#,
                422,
                "invalid",
            ),
            (
                "PUT",
                "/api/v1/pages/0/keys/40",
                r#"{"label":"x"}"#,
                422,
                "invalid",
            ),
            (
                "PUT",
                "/api/v1/pages/Nope/keys/4",
                r#"{"label":"x"}"#,
                404,
                "no-such-page",
            ),
            ("POST", "/api/v1/pages", r#"{"title":"x"}"#, 400, "bad-json"),
            ("DELETE", "/api/v1/pages/0", "", 409, "last-page"),
            (
                "PUT",
                "/api/v1/brightness",
                r#"{"value":300}"#,
                400,
                "bad-json",
            ),
            (
                "PUT",
                "/api/v1/brightness",
                r#"{"value":101}"#,
                422,
                "invalid",
            ),
        ];
        for (method, path, body, status, name) in cases {
            let response = fx.call(method, path, body);
            assert_eq!(
                (response.status, response.body["error"].as_str()),
                (status, Some(name)),
                "{method} {path} {body}: {}",
                response.body
            );
            assert!(response.body["detail"]
                .as_str()
                .is_some_and(|d| !d.is_empty()));
        }
        assert_eq!(fx.file(), START);
        assert!(fx.control.try_recv().is_err());
    }

    #[test]
    fn accepts_a_body_key_equal_to_the_route() {
        let fx = fixture("samekey", START);
        assert_eq!(
            fx.call("PUT", "/api/v1/pages/0/keys/4", r#"{"key":4,"label":"x"}"#)
                .status,
            200
        );
    }

    #[test]
    fn reads_an_absent_config_as_empty_and_names_an_unreadable_one() {
        let fx = fixture("absent", START);
        std::fs::remove_file(&fx.api.config_path).unwrap();
        assert_eq!(
            fx.call("GET", "/api/v1/layout", "").body["pages"][0]["buttons"],
            json!([])
        );
        std::fs::write(&fx.api.config_path, "this is = = not toml").unwrap();
        let response = fx.call("GET", "/api/v1/layout", "");
        assert_eq!(
            (response.status, response.body["error"].as_str()),
            (500, Some("unreadable"))
        );
    }

    #[test]
    fn names_a_write_that_fails_and_keeps_the_old_file() {
        let fx = fixture("writefail", START);
        let blocker = fx.api.config_path.with_extension("toml.tmp");
        std::fs::create_dir_all(&blocker).unwrap();
        let response = fx.call("PUT", "/api/v1/brightness", r#"{"value":70}"#);
        assert_eq!(
            (response.status, response.body["error"].as_str()),
            (500, Some("write-failed"))
        );
        assert_eq!(fx.file(), START);
    }
}
