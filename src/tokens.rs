//! Caller tokens for the control API (`docs/control-api.md`, *Tokens*).
//!
//! One file per caller, `<dir>/<caller>`, holding `sdk_<caller>_<32 hex>`.
//! The store is read on every check, so a revoked token is refused at once.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// A directory of caller tokens.
#[derive(Debug, Clone)]
pub struct TokenStore {
    dir: PathBuf,
}

/// The token directory under `$XDG_CONFIG_HOME` (or `~/.config`).
pub fn default_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("streamdeck").join("tokens")
}

/// Whether `name` may name a caller: 1-32 of `a-z`, `0-9`, `-`, not starting with `-`.
pub fn is_caller_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl TokenStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Mint (or replace) `caller`'s token; also write it to `out` when given.
    /// The token is never returned, so it is never printed.
    pub fn issue(&self, caller: &str, out: Option<&Path>) -> io::Result<PathBuf> {
        if !is_caller_name(caller) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("'{caller}' is not a caller name (a-z, 0-9, -)"),
            ));
        }
        fs::create_dir_all(&self.dir)?;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        let token = format!("sdk_{caller}_{}", random_hex(16)?);
        let path = self.dir.join(caller);
        write_private(&path, &token)?;
        if let Some(out) = out {
            write_private(out, &token)?;
        }
        Ok(path)
    }

    /// Delete `caller`'s token; `false` when it held none.
    pub fn revoke(&self, caller: &str) -> io::Result<bool> {
        if !is_caller_name(caller) {
            return Ok(false);
        }
        match fs::remove_file(self.dir.join(caller)) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// The callers holding a token, sorted.
    pub fn list(&self) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let mut callers: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| is_caller_name(name))
            .collect();
        callers.sort();
        Ok(callers)
    }

    /// The caller whose token `presented` is, if any.
    pub fn caller_of(&self, presented: &str) -> Option<String> {
        let presented = presented.trim();
        if presented.is_empty() {
            return None;
        }
        let mut found = None;
        for caller in self.list().ok()? {
            let Ok(held) = fs::read_to_string(self.dir.join(&caller)) else {
                continue;
            };
            if constant_time_eq(held.trim().as_bytes(), presented.as_bytes()) {
                found = Some(caller);
            }
        }
        found
    }

    /// Read `caller`'s token, for a client on this machine.
    pub fn read(&self, caller: &str) -> io::Result<String> {
        Ok(fs::read_to_string(self.dir.join(caller))?
            .trim()
            .to_string())
    }
}

fn write_private(path: &Path, token: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

fn random_hex(bytes: usize) -> io::Result<String> {
    let mut buf = vec![0u8; bytes];
    fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> TokenStore {
        let dir = std::env::temp_dir().join(format!("sd-tokens-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TokenStore::new(dir.join("tokens"))
    }

    #[test]
    fn issues_a_private_token_that_names_its_caller() {
        let store = store("issue");
        let path = store.issue("runa", None).unwrap();
        let token = store.read("runa").unwrap();
        assert!(token.starts_with("sdk_runa_") && token.len() == "sdk_runa_".len() + 32);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(store.dir()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(store.caller_of(&token).as_deref(), Some("runa"));
    }

    #[test]
    fn writes_the_same_token_to_an_out_file() {
        let store = store("out");
        let out = store.dir().parent().unwrap().join("copy");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        store.issue("runa", Some(&out)).unwrap();
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            store.read("runa").unwrap()
        );
        assert_eq!(
            fs::metadata(&out).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn refuses_a_revoked_reissued_or_unknown_token() {
        let store = store("revoke");
        store.issue("cli", None).unwrap();
        let old = store.read("cli").unwrap();
        store.issue("cli", None).unwrap();
        assert_eq!(store.caller_of(&old), None);
        let new = store.read("cli").unwrap();
        assert!(store.revoke("cli").unwrap());
        assert_eq!(store.caller_of(&new), None);
        assert!(!store.revoke("cli").unwrap());
        assert_eq!(store.caller_of(""), None);
        assert_eq!(store.caller_of("sdk_cli_nope"), None);
    }

    #[test]
    fn lists_callers_and_reads_an_absent_store_as_empty() {
        let store = store("list");
        assert_eq!(store.list().unwrap(), Vec::<String>::new());
        store.issue("runa", None).unwrap();
        store.issue("cli", None).unwrap();
        assert_eq!(store.list().unwrap(), vec!["cli", "runa"]);
    }

    #[test]
    fn accepts_only_plain_caller_names() {
        for good in ["cli", "runa", "home-assistant", "a1"] {
            assert!(is_caller_name(good), "{good}");
        }
        for bad in ["", "-x", "Runa", "../x", "a b", &"a".repeat(33)] {
            assert!(!is_caller_name(bad), "{bad}");
        }
        assert!(store("bad").issue("../x", None).is_err());
    }
}
