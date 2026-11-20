use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Result of loading a persisted JSON file with automatic backup recovery,
/// I/O-error separation, and non-destructive corruption quarantine.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadOutcome<T> {
    Loaded(T),
    RecoveredFromBackup(T),
    Missing,
    /// Primary file existed and was read, but failed JSON parsing or schema validation
    /// and no valid `.bak` was available. Corrupted file is quarantined once (moved).
    Corrupted {
        quarantined: Option<PathBuf>,
    },
    /// Transient or permission I/O failure prevented reading; file was NOT treated as
    /// corrupted and was NOT quarantined or overwritten.
    IoError,
}

impl<T> LoadOutcome<T> {
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Loaded(v) | Self::RecoveredFromBackup(v) => Some(v),
            Self::Missing | Self::Corrupted { .. } | Self::IoError => None,
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

fn is_transient_win_err(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::AlreadyExists
    ) || e.raw_os_error() == Some(32)
        || e.raw_os_error() == Some(5)
}

fn read_to_string_with_retry(path: &Path) -> io::Result<String> {
    let mut last_err = None;
    for attempt in 0..4 {
        match fs::read_to_string(path) {
            Ok(s) => return Ok(s),
            Err(e) => {
                let retryable = is_transient_win_err(&e);
                last_err = Some(e);
                if !retryable || attempt == 3 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("read failed")))
}

fn remove_file_with_retry(path: &Path) -> io::Result<()> {
    let mut last_err = None;
    for attempt in 0..5 {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                let retryable = is_transient_win_err(&e);
                last_err = Some(e);
                if !retryable || attempt == 4 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("remove failed")))
}

fn rename_with_retry(src: &Path, dst: &Path) -> io::Result<()> {
    let mut last_err = None;
    for attempt in 0..6 {
        match fs::rename(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let retryable = is_transient_win_err(&e);
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

/// Moves a corrupted file out of the way to `<file>.corrupt-<ts>-<seq>` (BH-35)
/// so repeated loads never spam duplicate `.corrupt-*` copies.
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
    if rename_with_retry(path, &dst).is_ok() {
        return Some(dst);
    }
    // Fallback to a single deterministic `.corrupt` copy if rename failed.
    let fallback = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.join(format!("{}.corrupt", file_name))
        }
        _ => PathBuf::from(format!("{}.corrupt", file_name)),
    };
    if fs::copy(path, &fallback).is_ok() {
        Some(fallback)
    } else {
        None
    }
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
///    passing `backup_eligible`, saves a durable `.bak` copy before replacing `path`.
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
        if let Ok(bak_tmp) = write_temp_and_sync(&bak, &existing)
            && rename_with_retry(&bak_tmp, &bak).is_err()
        {
            let _ = fs::remove_file(&bak_tmp);
        }
    }

    if let Err(e) = rename_with_retry(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Atomically writes pre-serialized JSON string `json` to `path`.
/// Validates incoming JSON syntax and never overwrites `.bak` with invalid JSON.
pub fn atomic_write_json_str(path: &Path, json: &str, keep_backup: bool) -> io::Result<()> {
    serde_json::from_str::<serde_json::Value>(json)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write_bytes_guarded(path, json.as_bytes(), keep_backup, |existing| {
        serde_json::from_slice::<serde_json::Value>(existing).is_ok()
    })
}

/// Serializes `value` to pretty JSON *before* touching the filesystem, then writes atomically.
/// Never overwrites `.bak` with an existing primary file that is not valid JSON.
pub fn atomic_write_json<T: Serialize>(
    path: &Path,
    value: &T,
    keep_backup: bool,
) -> io::Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write_json_str(path, &json, keep_backup)
}

/// Serializes `value` to pretty JSON and validates any existing primary file against schema `T`
/// and `validate` before rotating it into `.bak` (prevents schema-invalid `.bak` poisoning — BH-35).
pub fn atomic_write_json_validated<T, F>(
    path: &Path,
    value: &T,
    keep_backup: bool,
    validate: F,
) -> io::Result<()>
where
    T: Serialize + DeserializeOwned,
    F: Fn(&T) -> bool,
{
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write_bytes_guarded(path, json.as_bytes(), keep_backup, |existing| {
        serde_json::from_slice::<T>(existing)
            .map(|parsed| validate(&parsed))
            .unwrap_or(false)
    })
}

enum ParseFileResult<T> {
    Ok(T),
    NotFound,
    IoErr,
    Corrupt,
}

fn read_and_validate<T, F>(path: &Path, validate: &F) -> ParseFileResult<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> bool,
{
    let text = match read_to_string_with_retry(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return ParseFileResult::NotFound,
        Err(_) => return ParseFileResult::IoErr,
    };
    match serde_json::from_str::<T>(&text) {
        Ok(parsed) if validate(&parsed) => ParseFileResult::Ok(parsed),
        _ => ParseFileResult::Corrupt,
    }
}

/// Loads and validates JSON from `path`:
/// - Distinguishes transient/permission I/O errors (`LoadOutcome::IoError`) from corruption.
/// - On parse/validation failure, moves the corrupt primary to `.corrupt-<ts>-<seq>` (preventing
///   unbounded quarantine spam on reloads), recovers from `<path>.bak` if valid, and heals
///   `path` from `.bak` so subsequent reads succeed directly.
pub fn load_json_with_recovery<T, F>(path: &Path, validate: F) -> LoadOutcome<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> bool,
{
    let bak = backup_path(path);
    match read_and_validate(path, &validate) {
        ParseFileResult::Ok(v) => LoadOutcome::Loaded(v),
        ParseFileResult::IoErr => LoadOutcome::IoError,
        ParseFileResult::NotFound => match read_and_validate(&bak, &validate) {
            ParseFileResult::Ok(v) => {
                if let Ok(bak_bytes) = fs::read(&bak) {
                    let _ = atomic_write_bytes(path, &bak_bytes, false);
                }
                LoadOutcome::RecoveredFromBackup(v)
            }
            ParseFileResult::NotFound => LoadOutcome::Missing,
            ParseFileResult::IoErr => LoadOutcome::IoError,
            ParseFileResult::Corrupt => {
                let quarantined = quarantine_corrupt_file(&bak);
                LoadOutcome::Corrupted { quarantined }
            }
        },
        ParseFileResult::Corrupt => {
            let quarantined = quarantine_corrupt_file(path);
            match read_and_validate(&bak, &validate) {
                ParseFileResult::Ok(v) => {
                    if let Ok(bak_bytes) = fs::read(&bak) {
                        let _ = atomic_write_bytes(path, &bak_bytes, false);
                    }
                    LoadOutcome::RecoveredFromBackup(v)
                }
                _ => LoadOutcome::Corrupted { quarantined },
            }
        }
    }
}

/// Removes `<path>.bak` FIRST and then `path` so a failed `.bak` deletion can never
/// cause zombie resurrection of deleted state on the next load (BH-35).
pub fn remove_with_backup(path: &Path) -> io::Result<()> {
    let bak = backup_path(path);
    remove_file_with_retry(&bak)?;
    remove_file_with_retry(path)?;
    Ok(())
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
        assert_eq!(
            load_json_with_recovery::<SampleState, _>(&backup_path(&path), |_| true),
            LoadOutcome::Loaded(v1)
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_primary_recovers_heals_and_does_not_spam_quarantine_files() {
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

        fs::write(&path, "{ \"version\": 2, \"name\": ").unwrap();

        let outcome = load_json_with_recovery::<SampleState, _>(&path, |s| s.version > 0);
        assert_eq!(outcome, LoadOutcome::RecoveredFromBackup(v1.clone()));

        // Second load must hit Loaded(v1) directly (primary healed) with still only 1 quarantine file.
        let second = load_json_with_recovery::<SampleState, _>(&path, |s| s.version > 0);
        assert_eq!(second, LoadOutcome::Loaded(v1));

        let quarantine_count = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .count();
        assert_eq!(quarantine_count, 1, "must create exactly 1 quarantine file");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_primary_without_backup_quarantines_once_without_spam() {
        let dir = temp_test_dir("corrupt_nobak");
        let path = dir.join("shelf.json");
        fs::write(&path, "NOT VALID JSON {{{{").unwrap();

        let outcome = load_json_with_recovery::<SampleState, _>(&path, |_| true);
        match outcome {
            LoadOutcome::Corrupted { quarantined } => {
                let q = quarantined.expect("quarantined file must exist");
                assert!(q.exists());
                assert_eq!(fs::read_to_string(&q).unwrap(), "NOT VALID JSON {{{{");
            }
            other => panic!("expected Corrupted, got {:?}", other),
        }

        // Subsequent load sees Missing (corrupt file moved to quarantine) — no disk spam!
        assert_eq!(
            load_json_with_recovery::<SampleState, _>(&path, |_| true),
            LoadOutcome::Missing
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn directory_io_error_is_not_misclassified_as_corrupted() {
        let dir = temp_test_dir("io_err");
        let outcome = load_json_with_recovery::<SampleState, _>(&dir, |_| true);
        assert_eq!(outcome, LoadOutcome::IoError);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validation_failure_does_not_poison_backup_when_using_validated_write() {
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
        let v3 = SampleState {
            version: 3,
            name: "v3".to_string(),
        };
        atomic_write_json_validated(&path, &good, true, |s| s.version > 0).unwrap();
        atomic_write_json_validated(&path, &bad, true, |s| s.version > 0).unwrap();
        // Backup holds `good`; writing v3 must NOT overwrite `.bak` with invalid `bad`.
        atomic_write_json_validated(&path, &v3, true, |s| s.version > 0).unwrap();
        assert_eq!(
            load_json_with_recovery::<SampleState, _>(&backup_path(&path), |s| s.version > 0),
            LoadOutcome::Loaded(good)
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
