//! The CLI's layout verbs: a client of the running daemon's control API
//! (`docs/control-api.md`, *The CLI client*).
//!
//! [`plan`] turns words into one request; [`send`] carries it over loopback
//! HTTP; [`judge`] reads the answer. Exit codes: `0` done, `1` failed before
//! the daemon answered, `2` the daemon refused.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Value};

/// The daemon's address when nothing names another.
pub const DEFAULT_URL: &str = "http://127.0.0.1:57617";

/// How long a call may take before it is `unreachable`.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The verbs this module answers; `main` routes these words here.
pub const VERBS: &[&str] = &["layout", "schema", "view", "key", "page"];

/// One request to the control API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub method: &'static str,
    pub path: String,
    /// The body; `Some("-")` means read it from standard input.
    pub body: Option<String>,
    pub revision: Option<String>,
    pub url: Option<String>,
}

/// A failure before the daemon answered, or the daemon's refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub name: String,
    pub detail: String,
}

impl Problem {
    pub fn new(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            detail: detail.into(),
        }
    }

    /// `{ "error", "detail" }`, as the daemon writes a refusal.
    pub fn to_json(&self) -> String {
        json!({ "error": self.name, "detail": self.detail }).to_string()
    }
}

fn usage(detail: impl Into<String>) -> Problem {
    Problem::new("usage", detail)
}

/// Turn the words after `streamdeck` into one call.
pub fn plan(args: &[String]) -> Result<Call, Problem> {
    let mut words = Vec::new();
    let mut revision = None;
    let mut url = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--revision" => {
                revision = Some(
                    iter.next()
                        .ok_or_else(|| usage("--revision needs a value"))?
                        .clone(),
                )
            }
            "--url" => {
                url = Some(
                    iter.next()
                        .ok_or_else(|| usage("--url needs a value"))?
                        .clone(),
                )
            }
            flag if flag.starts_with("--") => return Err(usage(format!("unknown option {flag}"))),
            word => words.push(word),
        }
    }

    let call = |method, path: String, body: Option<String>| Call {
        method,
        path,
        body,
        revision: revision.clone(),
        url: url.clone(),
    };
    match words.as_slice() {
        ["layout"] => Ok(call("GET", "/api/v1/layout".into(), None)),
        ["schema"] => Ok(call("GET", "/api/v1/schema".into(), None)),
        ["view"] => Ok(call("GET", "/api/v1/view".into(), None)),
        ["layout", "brightness", value] => {
            let value: u8 = value
                .parse()
                .map_err(|_| usage(format!("brightness '{value}' is not 0-100")))?;
            Ok(call("PUT", "/api/v1/brightness".into(), Some(json!({ "value": value }).to_string())))
        }
        ["key", "set", page, key, button] => {
            let key = parse_key(key)?;
            if *button != "-" {
                let value: Value = serde_json::from_str(button)
                    .map_err(|e| usage(format!("the button is not JSON: {e}")))?;
                if !value.is_object() {
                    return Err(usage("the button is a JSON object"));
                }
            }
            let path = format!("/api/v1/pages/{}/keys/{key}", encode(page));
            Ok(call("PUT", path, Some((*button).to_string())))
        }
        ["key", "clear", page, key] => {
            let key = parse_key(key)?;
            Ok(call("DELETE", format!("/api/v1/pages/{}/keys/{key}", encode(page)), Some("{}".into())))
        }
        ["page", "add", name] => Ok(call("POST", "/api/v1/pages".into(), Some(json!({ "name": name }).to_string()))),
        ["page", "remove", page] => Ok(call("DELETE", format!("/api/v1/pages/{}", encode(page)), Some("{}".into()))),
        _ => Err(usage(
            "streamdeck layout | schema | view | layout brightness <0-100> | key set <page> <key> <json|-> | \
             key clear <page> <key> | page add <name> | page remove <page>  [--revision <rev>] [--url <url>]",
        )),
    }
}

fn parse_key(word: &str) -> Result<u8, Problem> {
    word.parse()
        .map_err(|_| usage(format!("key '{word}' is not a key index (0-255)")))
}

/// Percent-encode one path segment.
fn encode(segment: &str) -> String {
    segment
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `host:port` from an `http://` URL.
pub fn authority(url: &str) -> Result<String, Problem> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| usage(format!("'{url}' is not an http:// URL")))?;
    let authority = rest.trim_end_matches('/');
    if authority.is_empty() || authority.contains('/') || !authority.contains(':') {
        return Err(usage(format!("'{url}' is not http://<host>:<port>")));
    }
    Ok(authority.to_string())
}

/// Carry `call` (with its body resolved) to the daemon; answer status and body.
pub fn send(
    authority: &str,
    token: &str,
    call: &Call,
    body: Option<&str>,
) -> Result<(u16, String), Problem> {
    let unreachable = |e: std::io::Error| Problem::new("unreachable", format!("{authority}: {e}"));
    let addr = std::net::ToSocketAddrs::to_socket_addrs(authority)
        .map_err(unreachable)?
        .next()
        .ok_or_else(|| Problem::new("unreachable", format!("{authority}: no address")))?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(unreachable)?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(unreachable)?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(unreachable)?;

    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n",
        call.method, call.path
    );
    if let Some(revision) = &call.revision {
        head.push_str(&format!("If-Match: \"{revision}\"\r\n"));
    }
    let body = body.unwrap_or("");
    if call.method != "GET" {
        head.push_str("Content-Type: application/json\r\n");
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(head.as_bytes()).map_err(unreachable)?;
    stream.write_all(body.as_bytes()).map_err(unreachable)?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(unreachable)?;
    parse_response(&raw)
}

/// Split a raw HTTP/1.1 response into status and body, de-chunking if needed.
pub fn parse_response(raw: &[u8]) -> Result<(u16, String), Problem> {
    let bad = |why: &str| Problem::new("unreachable", format!("not an HTTP answer: {why}"));
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| bad("no header end"))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad("no status"))?;
    let chunked = head
        .lines()
        .any(|l| l.to_ascii_lowercase().replace(' ', "") == "transfer-encoding:chunked");
    let body = if chunked {
        dechunk(body)
    } else {
        body.to_string()
    };
    Ok((status, body))
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some((size, after)) = rest.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.trim(), 16) else {
            break;
        };
        if size == 0 || after.len() < size {
            break;
        }
        out.push_str(&after[..size]);
        rest = after[size..].trim_start_matches("\r\n");
    }
    out
}

/// Read the daemon's answer: `Ok` with its JSON pretty-printed, or its refusal.
pub fn judge(status: u16, body: &str) -> Result<String, Problem> {
    let value: Value = serde_json::from_str(body).map_err(|_| {
        Problem::new(
            "unreachable",
            format!("{status}: not the control API's answer: {body}"),
        )
    })?;
    if status == 200 {
        return Ok(serde_json::to_string_pretty(&value).unwrap_or_else(|_| body.to_string()));
    }
    let name = value["error"].as_str().unwrap_or("refused");
    let detail = value["detail"].as_str().unwrap_or("").to_string();
    Err(Problem::new(name, detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    #[test]
    fn plans_each_verb_as_its_route() {
        let cases = [
            ("layout", "GET", "/api/v1/layout", None),
            ("schema", "GET", "/api/v1/schema", None),
            ("view", "GET", "/api/v1/view", None),
            (
                "layout brightness 70",
                "PUT",
                "/api/v1/brightness",
                Some(r#"{"value":70}"#),
            ),
            (
                "key set Media 3 -",
                "PUT",
                "/api/v1/pages/Media/keys/3",
                Some("-"),
            ),
            (
                "key clear 0 3",
                "DELETE",
                "/api/v1/pages/0/keys/3",
                Some("{}"),
            ),
            (
                "page add Media",
                "POST",
                "/api/v1/pages",
                Some(r#"{"name":"Media"}"#),
            ),
            ("page remove 1", "DELETE", "/api/v1/pages/1", Some("{}")),
        ];
        for (line, method, path, body) in cases {
            let call = plan(&words(line)).unwrap();
            assert_eq!(
                (call.method, call.path.as_str(), call.body.as_deref()),
                (method, path, body),
                "{line}"
            );
        }
    }

    #[test]
    fn carries_a_button_revision_and_url_and_encodes_page_names() {
        let mut args = words("key set");
        args.extend(["My page/2".into(), "4".into(), r#"{"label":"x"}"#.into()]);
        args.extend(words("--revision abc --url http://127.0.0.1:1"));
        let call = plan(&args).unwrap();
        assert_eq!(call.path, "/api/v1/pages/My%20page%2F2/keys/4");
        assert_eq!(call.body.as_deref(), Some(r#"{"label":"x"}"#));
        assert_eq!(call.revision.as_deref(), Some("abc"));
        assert_eq!(call.url.as_deref(), Some("http://127.0.0.1:1"));
    }

    #[test]
    fn refuses_words_it_cannot_send_as_usage() {
        for line in [
            "nope",
            "key set 0 x {}",
            "key set 0 1 notjson",
            "key set 0 1 [1]",
            "key clear 0",
            "layout brightness loud",
            "layout --colour red",
            "layout --revision",
            "layout --url",
            "view now",
        ] {
            assert_eq!(plan(&words(line)).unwrap_err().name, "usage", "{line}");
        }
    }

    #[test]
    fn reads_an_http_url_as_its_authority() {
        assert_eq!(
            authority("http://127.0.0.1:57617").unwrap(),
            "127.0.0.1:57617"
        );
        assert_eq!(authority("http://localhost:1/").unwrap(), "localhost:1");
        for bad in [
            "https://127.0.0.1:1",
            "127.0.0.1:1",
            "http://",
            "http://host",
            "http://h:1/x",
        ] {
            assert_eq!(authority(bad).unwrap_err().name, "usage", "{bad}");
        }
    }

    #[test]
    fn parses_plain_and_chunked_answers() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_response(plain).unwrap(), (200, "{}".into()));
        let chunked = b"HTTP/1.1 409 Conflict\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), (409, r#"{"a":1}"#.into()));
        assert_eq!(parse_response(b"garbage").unwrap_err().name, "unreachable");
        assert_eq!(
            parse_response(b"HTTP/1.1 x\r\n\r\n").unwrap_err().name,
            "unreachable"
        );
    }

    #[test]
    fn judges_an_answer_as_done_or_the_daemons_refusal() {
        assert_eq!(
            judge(200, r#"{"revision":"r"}"#).unwrap(),
            "{\n  \"revision\": \"r\"\n}"
        );
        let refused = judge(
            409,
            r#"{"error":"stale-revision","detail":"read it again"}"#,
        )
        .unwrap_err();
        assert_eq!(refused, Problem::new("stale-revision", "read it again"));
        assert_eq!(judge(404, "not found").unwrap_err().name, "unreachable");
        assert_eq!(
            Problem::new("a", "b").to_json(),
            r#"{"detail":"b","error":"a"}"#
        );
    }

    #[test]
    fn names_a_daemon_that_does_not_answer_as_unreachable() {
        let call = plan(&words("layout")).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let problem = send(&format!("127.0.0.1:{port}"), "t", &call, None).unwrap_err();
        assert_eq!(problem.name, "unreachable");
    }
}
