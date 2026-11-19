//! Extra country profiles (docs/country-profiles.md, "Extra profiles"): files the installation adds
//! to the ones in the app.
//!
//! Two places feed them, both optional:
//! * the folder `jurisdictions` in the app's data folder (an administrator can put `<ID>.toml` files
//!   there, or mount the folder on a managed machine);
//! * the signed-in server's `JURISDICTIONS_DIR`, downloaded at the start and every few hours and kept
//!   in `jurisdictions/.server` so they also work offline.
//!
//! A file with the id of a bundled profile replaces it. A file that is not valid is skipped and
//! reported in the log; the others are used.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Manager};
use zeima_core::jurisdiction;

use crate::account::{self, discover};
use crate::state::AppState;

const REFRESH_EVERY: Duration = Duration::from_secs(6 * 3600);
const CACHE_DIR: &str = ".server";

fn local_dir(app: &AppHandle) -> Option<PathBuf> {
    crate::instance::data_dir(app).ok().map(|d| d.join("jurisdictions"))
}

fn is_profile_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".toml") else { return false };
    !stem.ends_with(".fixtures") && !stem.is_empty() && !stem.starts_with('.')
}

/// The `*.toml` profile files of a folder (name, text), by name.
pub(crate) fn read_files(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let meta = e.metadata().ok()?;
            if !(meta.is_file() && meta.len() <= 256 * 1024 && is_profile_file(&name)) {
                return None;
            }
            let text = std::fs::read_to_string(e.path()).ok()?;
            Some((name, text))
        })
        .collect();
    out.sort();
    out
}

/// Reads both folders and makes the profiles available. Files in the data folder win over the
/// server's when both have the same id (the person's administrator knows best).
pub fn load(app: &AppHandle) {
    let Some(dir) = local_dir(app) else { return };
    // What the server offers may add countries, but not rewrite the law the app ships (hours, breaks, holidays): that
    // would let a server silently weaken the compliance checks. Replacing a bundled profile takes a file in the
    // folder on this machine, which the person's administrator controls.
    let (mut files, refused) = without_replacements(read_files(&dir.join(CACHE_DIR)));
    for name in refused {
        eprintln!("country profile of the server ignored: {name} would replace a profile that ships with the app");
    }
    files.extend(read_files(&dir));
    let problems = jurisdiction::set_extra(&dedup_last_wins(files));
    for p in problems {
        eprintln!("country profile skipped: {p}");
    }
}

/// Splits off the files whose profile has the id of a bundled one (and the names of those).
fn without_replacements(files: Vec<(String, String)>) -> (Vec<(String, String)>, Vec<String>) {
    let mut kept = Vec::new();
    let mut refused = Vec::new();
    for (name, src) in files {
        match jurisdiction::parse(&src) {
            Ok(def) if jurisdiction::is_bundled(&def.id) => refused.push(name),
            _ => kept.push((name, src)), // an invalid file is reported when it is loaded
        }
    }
    (kept, refused)
}

/// When two files claim the same id the later one stays; `set_extra` would report both as a clash.
fn dedup_last_wins(files: Vec<(String, String)>) -> Vec<(String, String)> {
    let id_of = |src: &str| jurisdiction::parse(src).ok().map(|d| d.id);
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, src) in files {
        if let Some(id) = id_of(&src) {
            out.retain(|(_, s)| id_of(s).as_deref() != Some(id.as_str()));
        }
        out.push((name, src));
    }
    out
}

#[derive(Deserialize)]
struct Served {
    profiles: Vec<ServedFile>,
}

#[derive(Deserialize)]
struct ServedFile {
    file: String,
    source: String,
}

/// Downloads the server's profiles into the cache folder (replacing what was there).
fn fetch(server: &str, cache: &Path) -> Result<usize, String> {
    let (discovery, _) = discover(server)?;
    let body = account::http_get(&format!("{}/v1/jurisdictions", discovery.api_url.trim_end_matches('/')))?;
    let served: Served = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    for (name, _) in read_files(cache) {
        let _ = std::fs::remove_file(cache.join(name));
    }
    let mut written = 0;
    for f in served.profiles.into_iter().take(50) {
        // The server's names are not trusted as paths.
        if f.file.contains(['/', '\\']) || !is_profile_file(&f.file) || f.source.len() > 256 * 1024 {
            continue;
        }
        std::fs::write(cache.join(&f.file), f.source).map_err(|e| e.to_string())?;
        written += 1;
    }
    Ok(written)
}

async fn refresh(app: &AppHandle) {
    let Some(dir) = local_dir(app) else { return };
    let state = app.state::<AppState>();
    let server = account::server_url(&state).await.ok().flatten();
    if let Some(server) = server {
        let cache = dir.join(CACHE_DIR);
        match tauri::async_runtime::spawn_blocking(move || fetch(&server, &cache)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => eprintln!("country profiles of the server: {e}"),
            Err(e) => eprintln!("country profiles of the server: {e}"),
        }
    }
    load(app);
}

/// Loads what is on disk at once, then keeps the server's profiles fresh.
pub fn spawn(app: AppHandle) {
    load(&app);
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app).await;
            tokio::time::sleep(REFRESH_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_profile_files_are_read() {
        assert!(is_profile_file("LU.toml"));
        assert!(!is_profile_file("LU.fixtures.toml"));
        assert!(!is_profile_file("notes.txt"));
        assert!(!is_profile_file(".toml"));
        assert!(!is_profile_file(".hidden.toml"));
    }

    #[test]
    fn a_server_cannot_replace_a_profile_that_ships_with_the_app() {
        let src = |id: &str| {
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/zeima-core/jurisdictions/AT.toml"))
                .unwrap()
                .replacen("id = \"AT\"", &format!("id = \"{id}\""), 1)
        };
        let (kept, refused) = without_replacements(vec![
            ("AT.toml".into(), src("AT")),
            ("LU.toml".into(), src("LU")),
            ("junk.toml".into(), "not a profile".into()),
        ]);
        assert_eq!(refused, vec!["AT.toml".to_string()]);
        assert_eq!(kept.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), vec!["LU.toml", "junk.toml"]);
    }

    #[test]
    fn reads_a_folder_sorted_and_skips_the_rest() {
        let dir = std::env::temp_dir().join(format!("zeima-extras-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub.toml")).unwrap();
        std::fs::write(dir.join("B.toml"), "b").unwrap();
        std::fs::write(dir.join("A.toml"), "a").unwrap();
        std::fs::write(dir.join("A.fixtures.toml"), "x").unwrap();
        std::fs::write(dir.join("readme.md"), "x").unwrap();
        let files = read_files(&dir);
        assert_eq!(files, vec![("A.toml".into(), "a".into()), ("B.toml".into(), "b".into())]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
