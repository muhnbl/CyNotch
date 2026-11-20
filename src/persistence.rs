use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Result of loading a persisted JSON file with automatic backup recovery
/// and non-destructive corruption quarantine.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadOutcome<T> {
    Loaded(T),
    RecoveredFromBackup(T),
    Missing,
    Corrupted { quarantined: Option<PathBuf> },
}

impl<T> LoadOutcome<T> {
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Loaded(v) | Self::RecoveredFromBackup(v) => Some(v),
            Self::Missing | Self::Corrupted { .. } => None,
        }
    }
}

/// Companion backup path (`<file>.bak`) stored alongside the primary file.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".bak");
    PathBuf::from(os)
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state".to_string());
    let tmp_name = format!(".{}.{}.{}.tmp", file_name, pid, seq);
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    }
}

fn quarantine_corrupt_file(path: &Path) -> Option<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state".to_string());
    let corrupt_name = format!("{}.corrupt-{}-{}", file_name, ts, seq);
    let dst = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(corrupt_name),
        _ => PathBuf::from(corrupt_name),
    };
    if fs::copy(path, &dst).is_ok() {
        Some(dst)
    } else {
        None
    }
}

fn rename_with_retry(src: &Path, dst: &Path) -> io::Result<()> {
    let mut last_err = None;
    for attempt in 0..6 {
        match fs::rename(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) => {
                // Transient Windows AV / indexer lock on destination file
                let retryable = matches!(
                    e.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::AlreadyExists
                ) || e.raw_os_error() == Some(32)
                    || e.raw_os_error() == Some(5);
                last_err = Some(e);
                if !retryable || attempt == 5 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("rename failed")))
}

fn write_temp_and_sync(path: &Path, data: &[u8]) -> io::Result<PathBuf> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let tmp = unique_temp_path(path);
    let write_res = (|| -> io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// Atomically writes `data` to `path`:
/// 1. Writes to a unique temporary file in the same directory and calls `sync_all()`.
/// 2. If `keep_backup` is true and `path` already exists with non-empty data
///    (passing `backup_eligible`), saves a durable `.bak` copy before replacing `path`.
/// 3. Atomically replaces `path` via `rename` (with transient Windows sharing-violation retry).
/// 4. Cleans up temp files on error without touching the existing valid destination.
pub fn atomic_write_bytes(path: &Path, data: &[u8], keep_backup: bool) -> io::Result<()> {
    atomic_write_bytes_guarded(path, data, keep_backup, |_| true)
}

fn atomic_write_bytes_guarded<F>(
    path: &Path,
    data: &[u8],
    keep_backup: bool,
    backup_eligible: F,
) -> io::Result<()>
where
    F: Fn(&[u8]) -> bool,
{
    let tmp = write_temp_and_sync(path, data)?;

    if keep_backup
        && let Ok(existing) = fs::read(path)
        && !existing.is_empty()
        && backup_eligible(&existing)
    {
        let bak = backup_path(path);
        if let Ok(bak_tmp) = write_temp_and_sync(&bak, &existing) {
            if rename_with_retry(&bak_tmp, &bak).is_err() {
                let _ = fs::remove_file(&bak_tmp);
            }
        }
    }

    if let Err(e) = rename_with_retry(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Atomically writes pre-serialized JSON string `json` to `path`.
/// Never overwrites `.bak` with an existing primary file that is not valid JSON.
pub fn atomic_write_json_str(path: &Path, json: &str, keep_backup: bool) -> io::Result<()> {
    atomic_write_bytes_guarded(path, json.as_bytes(), keep_backup, |existing| {
        serde_json::from_slice::<serde_json::Value>(existing).is_ok()
    })
}

/// Serializes `value` to pretty JSON *before* touching the filesystem, then writes atomically.
/// Never overwrites `.bak` with an existing primary file that is not valid JSON.
pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T, keep_backup: bool) -> io::Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write_json_str(path, &json, keep_backup)
}

/// Loads and validates JSON from `path`.
/// - If `path` is corrupted or fails `validate`, preserves a timestamped `.corrupt-*` copy
///   and attempts automatic recovery from `<path>.bak`.
/// - Never silently deletes or overwrites corrupted user data.
pub fn load_json_with_recovery<T, F>(path: &Path, validate: F) -> LoadOutcome<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> bool,
{
    let try_parse = |p: &Path| -> Option<T> {
        let text = fs::read_to_string(p).ok()?;
        let parsed = serde_json::from_str::<T>(&text).ok()?;
        if validate(&parsed) {
            Some(parsed)
        } else {
            None
        }
    };

    let bak = backup_path(path);
    if !path.exists() {
        return match try_parse(&bak) {
            Some(v) => LoadOutcome::RecoveredFromBackup(v),
            None => LoadOutcome::Missing,
        };
    }

    if let Some(v) = try_parse(path) {
        return LoadOutcome::Loaded(v);
    }

    let quarantined = quarantine_corrupt_file(path);
    if let Some(v) = try_parse(&bak) {
        LoadOutcome::RecoveredFromBackup(v)
    } else {
        LoadOutcome::Corrupted { quarantined }
    }
}

/// Removes `path` and its companion `.bak` file if present.
pub fn remove_with_backup(path: &Path) -> io::Result<()> {
    let mut primary_res = Ok(());
    if path.exists() {
        primary_res = fs::remove_file(path);
    }
    let bak = backup_path(path);
    if bak.exists() {
        let _ = fs::remove_file(&bak);
    }
    primary_res
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct SampleState {
        version: u32,
        name: String,
    }

    fn temp_test_dir(tag: &str) -> PathBuf {
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "cynotch_persist_test_{}_{}_{}",
            tag,
            std::process::id(),
            seq
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn atomic_write_and_load_roundtrip_preserves_backup() {
        let dir = temp_test_dir("roundtrip");
        let path = dir.join("config.json");

        let v1 = SampleState {
            version: 1,
            name: "first".to_string(),
        };
        let v2 = SampleState {
            version: 2,
            name: "second".to_string(),
        };

        atomic_write_json(&path, &v1, true).expect("write v1");
        assert_eq!(
            load_json_with_recovery::<SampleState, _>(&path, |s| s.version > 0),
            LoadOutcome::Loaded(v1.clone())
        );

        atomic_write_json(&path, &v2, true).expect("write v2");
        assert_eq!(
            load_json_with_recovery::<SampleState, _>(&path, |s| s.version > 0),
            LoadOutcome::Loaded(v2)
        );
        // .bak holds previous valid state v1
        assert_eq!(
            load_json_with_recovery::<SampleState, _>(&backup_path(&path), |_| true),
            LoadOutcome::Loaded(v1)
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_primary_recovers_from_backup_and_quarantines_corrupt_copy() {
        let dir = temp_test_dir("corrupt_recover");
        let path = dir.join("tasks.json");

        let v1 = SampleState {
            version: 1,
            name: "good_v1".to_string(),
        };
        let v2 = SampleState {
            version: 2,
            name: "good_v2".to_string(),
        };
        atomic_write_json(&path, &v1, true).unwrap();
        atomic_write_json(&path, &v2, true).unwrap();

        // Simulate interrupted/truncated write corrupting primary file
        fs::write(&path, "{ \"version\": 2, \"name\": ").unwrap();

        let outcome = load_json_with_recovery::<SampleState, _>(&path, |s| s.version > 0);
        assert_eq!(outcome, LoadOutcome::RecoveredFromBackup(v1));

        // Corrupt copy must be quarantined on disk
        let has_quarantine = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(has_quarantine, "corrupted file must be quarantined");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_primary_without_backup_returns_corrupted_and_preserves_file() {
        let dir = temp_test_dir("corrupt_nobak");
        let path = dir.join("shelf.json");
        fs::write(&path, "NOT VALID JSON {{{{").unwrap();

        let outcome = load_json_with_recovery::<SampleState, _>(&path, |_| true);
        match outcome {
            LoadOutcome::Corrupted { quarantined } => {
                let q = quarantined.expect("quarantined copy should exist");
                assert!(q.exists());
                assert_eq!(fs::read_to_string(&q).unwrap(), "NOT VALID JSON {{{{");
            }
            other => panic!("expected Corrupted, got {:?}", other),
        }
        // Original corrupted file is never silently deleted
        assert!(path.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validation_failure_triggers_backup_recovery() {
        let dir = temp_test_dir("validation");
        let path = dir.join("validated.json");

        let good = SampleState {
            version: 1,
            name: "ok".to_string(),
        };
        let bad = SampleState {
            version: 0,
            name: "invalid".to_string(),
        };
        atomic_write_json(&path, &good, true).unwrap();
        atomic_write_json(&path, &bad, true).unwrap();

        let outcome = load_json_with_recovery::<SampleState, _>(&path, |s| s.version > 0);
        assert_eq!(outcome, LoadOutcome::RecoveredFromBackup(good));

        let _ = fs::remove_dir_all(&dir);
    }
}
