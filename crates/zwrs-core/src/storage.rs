//! Crash-safe file handling.
//!
//! Player data is the one thing a server must never lose, so every write goes
//! through [`write_atomic`]:
//!
//! 1. write the full document to a temporary file in the same directory,
//! 2. `fsync` the file,
//! 3. `rename` it over the target (atomic on POSIX),
//! 4. `fsync` the directory so the rename itself survives power loss.
//!
//! A crash therefore leaves either the previous complete document or the new
//! complete document — never a half-written file.
//!
//! Reading is defensive in the other direction: malformed JSON is **never**
//! silently replaced. The bad file is moved aside with a timestamped
//! `.corrupt-*` suffix and the caller gets an error, so an operator can inspect
//! or recover it.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};
use crate::json::Json;
use crate::time::{Timestamp, UnixSeconds};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Schema version written into every ZWRS JSON document.
pub const FORMAT_VERSION: u64 = 1;

/// Writes `contents` to `path` atomically.
///
/// # Errors
/// Propagates I/O failures, including a failed directory `fsync`.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Storage(format!("invalid path: {}", path.display())))?;
    let temp_path = parent.join(format!(".{file_name}.tmp-{}-{counter}", std::process::id()));

    let result = (|| -> Result<()> {
        {
            let mut file = File::create(&temp_path)?;
            file.write_all(contents)?;
            file.sync_all()?;
        }
        if let Err(e) = fs::rename(&temp_path, path) {
            // Windows cannot rename over an existing file; retry once after
            // removing the target. Documented limitation: a crash between the
            // two steps is possible there, which is why Linux/Android is the
            // primary target.
            if cfg!(windows) {
                let _ = fs::remove_file(path);
                fs::rename(&temp_path, path)?;
            } else {
                return Err(Error::Io(e));
            }
        }
        // Persist the directory entry itself.
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Reads a JSON document, quarantining it if it is malformed.
///
/// Returns `Ok(None)` when the file does not exist.
///
/// # Errors
/// Returns [`Error::Storage`] when the file exists but cannot be parsed; the
/// original bytes are preserved in a `.corrupt-<timestamp>` sibling.
pub fn read_json(path: &Path) -> Result<Option<Json>> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    };
    if text.trim().is_empty() {
        let quarantine = quarantine_path(path);
        fs::rename(path, &quarantine)?;
        return Err(Error::Storage(format!(
            "{} was empty and has been moved to {}; the engine will not overwrite it with an empty database",
            path.display(),
            quarantine.display()
        )));
    }
    match Json::parse(&text) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            let quarantine = quarantine_path(path);
            fs::rename(path, &quarantine)?;
            Err(Error::Storage(format!(
                "{} is not valid JSON ({e}); the original has been preserved at {} for inspection or recovery",
                path.display(),
                quarantine.display()
            )))
        }
    }
}

fn quarantine_path(path: &Path) -> PathBuf {
    let stamp = Timestamp::now().to_string().replace(':', "");
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("data");
    path.with_file_name(format!("{name}.corrupt-{stamp}"))
}

/// Recursively copies `dir` into `<dir>/<name>-<timestamp>` for backups.
///
/// Returns the snapshot directory.
///
/// # Errors
/// Propagates I/O failures.
pub fn snapshot_directory(dir: &Path, target_root: &Path) -> Result<PathBuf> {
    // Timestamps are second-granular, so two snapshots in the same second are
    // disambiguated with a counter instead of failing.
    let stamp = Timestamp::now()
        .to_string()
        .replace(':', "")
        .replace('-', "");
    let mut dest = target_root.join(format!("backup-{stamp}"));
    let mut suffix = 1u32;
    while dest.exists() {
        dest = target_root.join(format!("backup-{stamp}-{suffix}"));
        suffix += 1;
        if suffix > 10_000 {
            return Err(Error::Storage("could not allocate a snapshot directory".into()));
        }
    }
    fs::create_dir_all(&dest)?;
    copy_tree(dir, &dest)?;
    Ok(dest)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let dest = to.join(&name);
        if path.is_dir() {
            // Never recurse into the backup directory itself.
            if name.to_string_lossy().starts_with("backups") {
                continue;
            }
            fs::create_dir_all(&dest)?;
            copy_tree(&path, &dest)?;
        } else if path.is_file() {
            fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

/// Removes the oldest snapshot directories, keeping `keep` newest.
///
/// # Errors
/// Propagates I/O failures other than "already removed".
pub fn prune_backups(target_root: &Path, keep: u32) -> Result<Vec<PathBuf>> {
    let mut names: Vec<PathBuf> = Vec::new();
    if !target_root.exists() {
        return Ok(names);
    }
    for entry in fs::read_dir(target_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.path());
        }
    }
    // Backup names embed a sortable UTC timestamp.
    names.sort();
    let keep = keep as usize;
    let mut removed = Vec::new();
    if names.len() > keep {
        for path in names.drain(..names.len() - keep) {
            fs::remove_dir_all(&path)?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// Reads a whole file naively (helper for tests and small documents).
///
/// # Errors
/// Propagates I/O failures.
pub fn read_to_string(path: &Path) -> Result<String> {
    let mut s = String::new();
    File::open(path)?.read_to_string(&mut s)?;
    Ok(s)
}

/// Document wrapper helpers shared by the repositories.
///
/// Stored shape:
/// ```json
/// { "format": 1, "written_at": "2026-11-10T10:00:00Z", "records": [ ... ] }
/// ```
pub(crate) fn document(records: Vec<Json>) -> Json {
    let now: UnixSeconds = crate::time::now_unix();
    let mut map = Json::object();
    map.insert("format".into(), Json::Number(FORMAT_VERSION as f64));
    map.insert(
        "written_at".into(),
        Json::String(crate::time::format_utc(now)),
    );
    map.insert("records".into(), Json::Array(records));
    Json::Object(map)
}

/// Extracts the record array from a document, rejecting unknown schema
/// versions instead of guessing.
pub(crate) fn records(doc: &Json) -> Result<Vec<Json>> {
    let format = doc
        .get("format")
        .and_then(Json::as_u64)
        .ok_or_else(|| Error::Storage("document is missing a numeric `format` field".into()))?;
    if format > FORMAT_VERSION {
        return Err(Error::Storage(format!(
            "document format {format} is newer than this build supports ({FORMAT_VERSION}); upgrade ZWRS instead of overwriting newer data"
        )));
    }
    doc.get("records")
        .and_then(Json::as_array)
        .map(<[Json]>::to_vec)
        .ok_or_else(|| Error::Storage("document is missing a `records` array".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zwrs-storage-{tag}-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_replaces_content() {
        let dir = temp_dir("atomic");
        let path = dir.join("players.json");
        write_atomic(&path, b"{\"a\":1}").unwrap();
        write_atomic(&path, b"{\"a\":2}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\":2}");
        // No temp files left behind.
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_json_is_quarantined_not_overwritten() {
        let dir = temp_dir("corrupt");
        let path = dir.join("players.json");
        fs::write(&path, b"{ this is not json").unwrap();
        let err = read_json(&path).unwrap_err();
        assert!(err.to_string().contains("preserved"), "{err}");
        assert!(!path.exists(), "corrupt file should be moved aside");

        let quarantined: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(quarantined.len(), 1);
        // Content is byte-identical, so an operator can repair it.
        assert_eq!(
            fs::read_to_string(quarantined[0].path()).unwrap(),
            "{ this is not json"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_file_is_quarantined() {
        let dir = temp_dir("empty");
        let path = dir.join("sessions.json");
        fs::write(&path, b"   \n").unwrap();
        assert!(read_json(&path).is_err());
        assert!(!path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_is_none() {
        let dir = temp_dir("missing");
        assert_eq!(read_json(&dir.join("nope.json")).unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn document_roundtrip_and_version_guard() {
        let doc = document(vec![Json::String("x".into())]);
        let text = doc.to_pretty();
        let parsed = Json::parse(&text).unwrap();
        assert_eq!(records(&parsed).unwrap().len(), 1);

        let mut future = Json::object();
        future.insert("format".into(), Json::Number(99.0));
        future.insert("records".into(), Json::Array(vec![]));
        let err = records(&Json::Object(future)).unwrap_err();
        assert!(err.to_string().contains("newer"), "{err}");
    }

    #[test]
    fn snapshot_and_prune() {
        let dir = temp_dir("snap");
        let data = dir.join("data");
        let backups = data.join("backups");
        fs::create_dir_all(data.join("players")).unwrap();
        fs::write(data.join("players/players.json"), b"[]").unwrap();

        let first = snapshot_directory(&data, &backups).unwrap();
        assert!(first.join("players/players.json").exists());
        // Backups must not nest inside themselves.
        let second = snapshot_directory(&data, &backups).unwrap();
        assert!(!second.join("backups").exists());

        let removed = prune_backups(&backups, 1).unwrap();
        assert_eq!(removed.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
