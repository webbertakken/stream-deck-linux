//! Who may talk to the editor's HTTP server.
//!
//! The server is on loopback, but a web page in the operator's browser can
//! still reach loopback: a cross-origin "simple" POST needs no CORS preflight,
//! and DNS rebinding makes a hostile name resolve to `127.0.0.1`. Either way a
//! page could write a key whose action runs a shell command. This guard admits
//! only requests addressed to the server by its loopback name, from its own
//! origin, and writes carried as JSON (which a cross-origin page cannot send
//! without a preflight this server never grants).

/// The parts of a request the guard decides on.
#[derive(Debug, Clone, Copy)]
pub struct RequestFacts<'a> {
    pub is_write: bool,
    pub host: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub content_type: Option<&'a str>,
}

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `Host` is missing or names something other than this server on loopback.
    ForeignHost,
    /// `Origin` is present and is not this server's own origin.
    ForeignOrigin,
    /// A write whose body is not declared as `application/json`.
    NotJson,
}

impl Refusal {
    /// The HTTP status the refusal answers with.
    pub fn status(self) -> u16 {
        match self {
            Refusal::ForeignHost | Refusal::ForeignOrigin => 403,
            Refusal::NotJson => 415,
        }
    }

    /// The refusal's name, as the response body and the log say it.
    pub fn name(self) -> &'static str {
        match self {
            Refusal::ForeignHost => "foreign-host",
            Refusal::ForeignOrigin => "foreign-origin",
            Refusal::NotJson => "not-json",
        }
    }
}

const LOOPBACK_NAMES: &[&str] = &["127.0.0.1", "localhost"];

/// Admit or refuse a request to the server listening on `port`.
pub fn admit(facts: &RequestFacts, port: u16) -> Result<(), Refusal> {
    let host_ok = facts
        .host
        .is_some_and(|host| is_loopback_authority(host, port));
    if !host_ok {
        return Err(Refusal::ForeignHost);
    }
    if let Some(origin) = facts.origin {
        let own = origin
            .strip_prefix("http://")
            .is_some_and(|authority| is_loopback_authority(authority, port));
        if !own {
            return Err(Refusal::ForeignOrigin);
        }
    }
    if facts.is_write && !is_json(facts.content_type) {
        return Err(Refusal::NotJson);
    }
    Ok(())
}

fn is_loopback_authority(authority: &str, port: u16) -> bool {
    let expected_port = port.to_string();
    authority
        .rsplit_once(':')
        .is_some_and(|(name, p)| p == expected_port && LOOPBACK_NAMES.contains(&name))
}

fn is_json(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: u16 = 57617;

    fn read(host: Option<&'static str>, origin: Option<&'static str>) -> RequestFacts<'static> {
        RequestFacts {
            is_write: false,
            host,
            origin,
            content_type: None,
        }
    }

    fn write(
        content_type: Option<&'static str>,
        origin: Option<&'static str>,
    ) -> RequestFacts<'static> {
        RequestFacts {
            is_write: true,
            host: Some("127.0.0.1:57617"),
            origin,
            content_type,
        }
    }

    #[test]
    fn admits_a_read_addressed_by_loopback_ip_or_localhost() {
        assert_eq!(admit(&read(Some("127.0.0.1:57617"), None), PORT), Ok(()));
        assert_eq!(admit(&read(Some("localhost:57617"), None), PORT), Ok(()));
    }

    #[test]
    fn refuses_a_rebound_or_missing_host() {
        assert_eq!(
            admit(&read(Some("evil.example:57617"), None), PORT),
            Err(Refusal::ForeignHost)
        );
        assert_eq!(
            admit(&read(Some("127.0.0.1"), None), PORT),
            Err(Refusal::ForeignHost)
        );
        assert_eq!(
            admit(&read(Some("127.0.0.1:80"), None), PORT),
            Err(Refusal::ForeignHost)
        );
        assert_eq!(admit(&read(None, None), PORT), Err(Refusal::ForeignHost));
    }

    #[test]
    fn refuses_a_foreign_origin_even_on_a_read() {
        let facts = read(Some("127.0.0.1:57617"), Some("https://evil.example"));
        assert_eq!(admit(&facts, PORT), Err(Refusal::ForeignOrigin));
        let null = read(Some("127.0.0.1:57617"), Some("null"));
        assert_eq!(admit(&null, PORT), Err(Refusal::ForeignOrigin));
    }

    #[test]
    fn admits_its_own_origin() {
        let facts = write(Some("application/json"), Some("http://127.0.0.1:57617"));
        assert_eq!(admit(&facts, PORT), Ok(()));
        let local = write(Some("application/json"), Some("http://localhost:57617"));
        assert_eq!(admit(&local, PORT), Ok(()));
    }

    #[test]
    fn admits_a_json_write_without_origin_from_a_program() {
        let facts = write(Some("application/json; charset=utf-8"), None);
        assert_eq!(admit(&facts, PORT), Ok(()));
    }

    #[test]
    fn refuses_a_write_that_is_not_json() {
        assert_eq!(
            admit(&write(Some("text/plain"), None), PORT),
            Err(Refusal::NotJson)
        );
        assert_eq!(admit(&write(None, None), PORT), Err(Refusal::NotJson));
    }

    #[test]
    fn names_each_refusal_with_its_status() {
        assert_eq!(
            (Refusal::ForeignHost.status(), Refusal::ForeignHost.name()),
            (403, "foreign-host")
        );
        assert_eq!(
            (
                Refusal::ForeignOrigin.status(),
                Refusal::ForeignOrigin.name()
            ),
            (403, "foreign-origin")
        );
        assert_eq!(
            (Refusal::NotJson.status(), Refusal::NotJson.name()),
            (415, "not-json")
        );
    }
}
