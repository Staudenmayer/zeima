//! Secrets in the OS keychain (macOS Keychain, Windows Credential Manager,
//! Secret Service on Linux) for addons: their own secrets and the refresh
//! tokens of connected OAuth accounts. Nothing secret goes into SQLite.
//!
//! Calls block (and may show an unlock prompt): run them on a blocking
//! thread.

use std::sync::Arc;

/// Keychain "service" all entries are filed under.
const SERVICE: &str = "com.zeima.zeima";
/// Windows limits an entry to 2,560 bytes; longer values (some refresh
/// tokens) are split over several entries.
const CHUNK_BYTES: usize = 2_000;
/// Largest value accepted.
pub const MAX_SECRET_BYTES: usize = 8 * 1024;

/// A keychain or, in tests, a map.
pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, name: &str) -> Result<(), String>;
}

pub type Secrets = Arc<dyn SecretStore>;

/// Name of an addon's keychain entry; `name` is `secret:<key>` or
/// `oauth:<provider>` (see `addon_secret`).
pub fn entry_name(addon_id: &str, name: &str) -> String {
    format!("addon:{addon_id}:{name}")
}

/// The OS keychain. The first entry holds `<chunks>\n<first chunk>`, the
/// others `<name>#1`, `<name>#2`, …
pub struct Keychain;

fn entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(&crate::instance::keychain_service(SERVICE), name)
        .map_err(|e| format!("keychain: {e}"))
}

fn read(name: &str) -> Result<Option<String>, String> {
    match entry(name)?.get_secret() {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "keychain: not text".to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keychain: {e}")),
    }
}

fn remove(name: &str) -> Result<(), String> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keychain: {e}")),
    }
}

fn chunk_name(name: &str, i: usize) -> String {
    format!("{name}#{i}")
}

/// Splits at char boundaries into pieces of at most `CHUNK_BYTES`.
fn chunks(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = value;
    while rest.len() > CHUNK_BYTES {
        let mut cut = CHUNK_BYTES;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    out.push(rest);
    out
}

impl Keychain {
    /// Chunks stored under `name`, from its header.
    fn chunk_count(name: &str) -> Result<usize, String> {
        Ok(read(name)?
            .and_then(|v| v.split_once('\n').and_then(|(n, _)| n.parse().ok()))
            .unwrap_or(1))
    }
}

impl SecretStore for Keychain {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        let Some(head) = read(name)? else {
            return Ok(None);
        };
        let (count, first) = head
            .split_once('\n')
            .and_then(|(n, rest)| Some((n.parse::<usize>().ok()?, rest)))
            .ok_or("keychain: unknown entry format")?;
        let mut value = first.to_string();
        for i in 1..count {
            value += &read(&chunk_name(name, i))?.ok_or("keychain: entry is incomplete")?;
        }
        Ok(Some(value))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        if value.len() > MAX_SECRET_BYTES {
            return Err(format!("secrets are limited to {} KB", MAX_SECRET_BYTES / 1024));
        }
        let old = Self::chunk_count(name).unwrap_or(1);
        let parts = chunks(value);
        for (i, part) in parts.iter().enumerate().skip(1) {
            entry(&chunk_name(name, i))?
                .set_secret(part.as_bytes())
                .map_err(|e| format!("keychain: {e}"))?;
        }
        // The header last: a failed write above leaves the old value whole.
        entry(name)?
            .set_secret(format!("{}\n{}", parts.len(), parts[0]).as_bytes())
            .map_err(|e| format!("keychain: {e}"))?;
        for i in parts.len()..old {
            remove(&chunk_name(name, i))?;
        }
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let count = Self::chunk_count(name).unwrap_or(1);
        remove(name)?;
        for i in 1..count {
            remove(&chunk_name(name, i))?;
        }
        Ok(())
    }
}

/// In-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemorySecrets(pub std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        if value.len() > MAX_SECRET_BYTES {
            return Err("too long".into());
        }
        self.0.lock().unwrap().insert(name.into(), value.into());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_split_at_char_boundaries() {
        assert_eq!(chunks(""), vec![""]);
        let long = "ä".repeat(CHUNK_BYTES); // 2 bytes each
        let parts = chunks(&long);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.len() <= CHUNK_BYTES));
        assert_eq!(parts.concat(), long);
    }

    /// Uses the real keychain: `cargo test -p zeima keychain -- --ignored`.
    #[test]
    #[ignore]
    fn keychain_round_trip() {
        let name = "addon:dev.zeima.test:secret:round-trip";
        let long = "x".repeat(5_000);
        Keychain.set(name, &long).unwrap();
        assert_eq!(Keychain.get(name).unwrap().as_deref(), Some(long.as_str()));
        Keychain.set(name, "short").unwrap();
        assert_eq!(Keychain.get(name).unwrap().as_deref(), Some("short"));
        assert_eq!(read(&chunk_name(name, 1)).unwrap(), None);
        Keychain.delete(name).unwrap();
        assert_eq!(Keychain.get(name).unwrap(), None);
        Keychain.delete(name).unwrap();
    }
}
