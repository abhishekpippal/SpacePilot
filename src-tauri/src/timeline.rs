use crate::models::*;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const SCHEMA_VERSION: u32 = 1;
const HISTORY_FILE: &str = "timeline-v1.json";
const LARGE_FILE_THRESHOLD: u64 = 100 * 1024 * 1024;
const MAX_DIRECTORIES: usize = 500;
const MAX_LARGE_FILES: usize = 2_000;

struct Timing(&'static str, std::time::Instant);
impl Timing {
    fn start(name: &'static str) -> Self {
        Self(name, std::time::Instant::now())
    }
}
impl Drop for Timing {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        eprintln!(
            "{} elapsed_ms={:.3}",
            self.0,
            self.1.elapsed().as_secs_f64() * 1000.0
        );
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct SnapshotCategory {
    name: String,
    bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct SnapshotDirectory {
    path: String,
    bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct SnapshotFile {
    path: String,
    name: String,
    size: u64,
    modified: Option<String>,
    category: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct StorageSnapshot {
    schema_version: u32,
    timestamp: String,
    scope_id: String,
    volume_root: String,
    volume_capacity: u64,
    used_bytes: u64,
    free_bytes: u64,
    scan_root: String,
    scanned_bytes: u64,
    file_count: u64,
    directory_count: u64,
    categories: Vec<SnapshotCategory>,
    #[serde(default)]
    developer_categories: Vec<SnapshotCategory>,
    directories: Vec<SnapshotDirectory>,
    large_files: Vec<SnapshotFile>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DeletionRecord {
    timestamp: String,
    scope_id: String,
    bytes: u64,
    item_count: u64,
}

#[derive(Serialize, Deserialize)]
struct TimelineHistory {
    #[serde(default = "schema_version")]
    schema_version: u32,
    #[serde(default = "default_retention_days")]
    retention_days: u32,
    #[serde(default)]
    snapshots: Vec<StorageSnapshot>,
    #[serde(default)]
    deletions: Vec<DeletionRecord>,
}

impl Default for TimelineHistory {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            retention_days: default_retention_days(),
            snapshots: vec![],
            deletions: vec![],
        }
    }
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_retention_days() -> u32 {
    90
}

fn normalize_path(value: &str) -> String {
    let normalized = value.replace('/', "\\");
    let trimmed = normalized.trim_end_matches('\\');
    if cfg!(windows) {
        trimmed.to_ascii_lowercase()
    } else {
        trimmed.to_string()
    }
}

pub fn scope_identity(volume_root: &str, scan_root: &str) -> String {
    let input = format!(
        "{}\n{}",
        normalize_path(volume_root),
        normalize_path(scan_root)
    );
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

fn history_path(directory: &Path) -> PathBuf {
    directory.join(HISTORY_FILE)
}

fn load(directory: &Path) -> (TimelineHistory, Vec<String>, u64) {
    let _timing = Timing::start("timeline.load_history");
    let path = history_path(directory);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (TimelineHistory::default(), vec![], 0)
        }
        Err(_) => {
            return (
                TimelineHistory::default(),
                vec![
                    "Timeline history could not be read; the existing file has been preserved."
                        .into(),
                ],
                0,
            )
        }
    };
    let size = bytes.len() as u64;
    let _parse = Timing::start("timeline.parse_history");
    match serde_json::from_slice::<TimelineHistory>(&bytes) {
        Ok(history) if history.schema_version == SCHEMA_VERSION => (history, vec![], size),
        Ok(_) => (
            TimelineHistory::default(),
            vec!["Timeline history uses an unsupported schema and was ignored.".into()],
            size,
        ),
        Err(_) => (
            TimelineHistory::default(),
            vec!["Timeline history was corrupted and was ignored safely.".into()],
            size,
        ),
    }
}

fn save(directory: &Path, history: &TimelineHistory) -> Result<u64, String> {
    save_cancellable(directory, history, &|| false)
}

fn save_cancellable(
    directory: &Path,
    history: &TimelineHistory,
    cancelled: &dyn Fn() -> bool,
) -> Result<u64, String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let timing = Timing::start("timeline.serialize");
    let bytes = serde_json::to_vec(history).map_err(|error| error.to_string())?;
    drop(timing);
    #[cfg(debug_assertions)]
    eprintln!(
        "timeline.history bytes={} snapshots={} deletions={}",
        bytes.len(),
        history.snapshots.len(),
        history.deletions.len()
    );
    let temporary = directory.join(format!("timeline-{}.tmp", uuid::Uuid::new_v4()));
    let outcome = (|| {
        use std::io::Write;
        let timing = Timing::start("timeline.write_temp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        drop(timing);
        if cancelled() {
            return Err("Timeline cancelled; completed scan remains available.".to_string());
        }
        let _timing = Timing::start("timeline.rename");
        fs::rename(&temporary, history_path(directory)).map_err(|e| e.to_string())
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    outcome?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
fn snapshot(
    summary: &ScanSummary,
    directories: &HashMap<String, FileEntry>,
    files: &[FileEntry],
    now: DateTime<Utc>,
) -> StorageSnapshot {
    snapshot_cancellable(summary, directories, files, now, &|| false).unwrap()
}

fn snapshot_cancellable(
    summary: &ScanSummary,
    directories: &HashMap<String, FileEntry>,
    files: &[FileEntry],
    now: DateTime<Utc>,
    cancelled: &dyn Fn() -> bool,
) -> Result<StorageSnapshot, String> {
    let _total = Timing::start("timeline.build_snapshot");
    let timing = Timing::start("timeline.developer_analysis");
    let developer = crate::developer_storage::analyze_cancellable(files, directories, cancelled)?;
    drop(timing);
    let timing = Timing::start("timeline.select_sort_directories");
    let mut important_directories: Vec<_> = directories
        .values()
        .filter(|directory| directory.path != summary.root && directory.size > 0)
        .collect();
    let compare = |left: &&FileEntry, right: &&FileEntry| {
        right
            .size
            .cmp(&left.size)
            .then_with(|| left.path.cmp(&right.path))
    };
    if important_directories.len() > MAX_DIRECTORIES {
        important_directories.select_nth_unstable_by(MAX_DIRECTORIES, compare);
        important_directories.truncate(MAX_DIRECTORIES);
    }
    important_directories.sort_unstable_by(compare);
    let important_directories = important_directories
        .into_iter()
        .map(|directory| SnapshotDirectory {
            path: directory.path.clone(),
            bytes: directory.size,
        })
        .collect();
    drop(timing);
    let timing = Timing::start("timeline.select_sort_large_files");
    let mut large_files: Vec<_> = files
        .iter()
        .filter(|file| file.size >= LARGE_FILE_THRESHOLD)
        .collect();
    if large_files.len() > MAX_LARGE_FILES {
        large_files.select_nth_unstable_by(MAX_LARGE_FILES, compare);
        large_files.truncate(MAX_LARGE_FILES);
    }
    large_files.sort_unstable_by(compare);
    let large_files = large_files
        .into_iter()
        .map(|file| SnapshotFile {
            path: file.path.clone(),
            name: file.name.clone(),
            size: file.size,
            modified: file.modified.clone(),
            category: file.category.clone(),
        })
        .collect();
    drop(timing);
    let _timing = Timing::start("timeline.build_categories_scope");
    if cancelled() {
        return Err("Timeline cancelled; completed scan remains available.".into());
    }
    Ok(StorageSnapshot {
        schema_version: SCHEMA_VERSION,
        timestamp: now.to_rfc3339(),
        scope_id: scope_identity(&summary.volume_root, &summary.root),
        volume_root: summary.volume_root.clone(),
        volume_capacity: summary.volume_capacity,
        used_bytes: summary.volume_used,
        free_bytes: summary.volume_free,
        scan_root: summary.root.clone(),
        scanned_bytes: summary.total_size,
        file_count: summary.file_count,
        directory_count: summary.directory_count,
        categories: summary
            .category_totals
            .iter()
            .map(|category| SnapshotCategory {
                name: category.category.clone(),
                bytes: category.size,
            })
            .collect(),
        developer_categories: developer
            .categories
            .into_iter()
            .map(|category| SnapshotCategory {
                name: category.category,
                bytes: category.size_bytes,
            })
            .collect(),
        directories: important_directories,
        large_files,
    })
}

fn retain(history: &mut TimelineHistory, retention_days: u32, now: DateTime<Utc>) {
    let days = retention_days.clamp(30, 180) as i64;
    let cutoff = now - Duration::days(days);
    history.snapshots.retain(|snapshot| {
        DateTime::parse_from_rfc3339(&snapshot.timestamp).is_ok_and(|timestamp| timestamp >= cutoff)
    });
    history.deletions.retain(|record| {
        DateTime::parse_from_rfc3339(&record.timestamp).is_ok_and(|timestamp| timestamp >= cutoff)
    });
    history
        .snapshots
        .sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let granular_cutoff = now - Duration::days(7);
    let mut per_day: HashMap<String, usize> = HashMap::new();
    history.snapshots = history
        .snapshots
        .drain(..)
        .rev()
        .filter(|snapshot| {
            let Ok(timestamp) = DateTime::parse_from_rfc3339(&snapshot.timestamp) else {
                return false;
            };
            let day = timestamp.format("%Y-%m-%d").to_string();
            let count = per_day.entry(day).or_default();
            let limit = if timestamp >= granular_cutoff { 4 } else { 1 };
            if *count >= limit {
                false
            } else {
                *count += 1;
                true
            }
        })
        .collect();
    history.snapshots.reverse();
    history
        .deletions
        .sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    if history.deletions.len() > 2_000 {
        history.deletions.drain(0..history.deletions.len() - 2_000);
    }
}

#[cfg(test)]
pub fn record_snapshot(
    directory: &Path,
    summary: &ScanSummary,
    directories: &HashMap<String, FileEntry>,
    files: &[FileEntry],
    retention_days: u32,
) -> Result<u64, String> {
    record_snapshot_cancellable(
        directory,
        summary,
        directories,
        files,
        retention_days,
        &|| false,
    )
}

pub fn record_snapshot_cancellable(
    directory: &Path,
    summary: &ScanSummary,
    directories: &HashMap<String, FileEntry>,
    files: &[FileEntry],
    retention_days: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<u64, String> {
    let _total = Timing::start("timeline.total");
    if summary.cancelled {
        return Err("Cancelled scans are not saved to Timeline.".into());
    }
    let now = Utc::now();
    let (mut history, warnings, _) = load(directory);
    if !warnings.is_empty() {
        return Err(warnings.join(" "));
    }
    history.schema_version = SCHEMA_VERSION;
    history.retention_days = retention_days.clamp(30, 180);
    let built = snapshot_cancellable(summary, directories, files, now, cancelled)?;
    #[cfg(debug_assertions)]
    {
        let _timing = Timing::start("timeline.measure_snapshot_payload");
        if let Ok(bytes) = serde_json::to_vec(&built) {
            eprintln!(
                "timeline.snapshot bytes={} directories={} large_files={}",
                bytes.len(),
                built.directories.len(),
                built.large_files.len()
            );
        }
    }
    history.snapshots.push(built);
    let retention_days = history.retention_days;
    let timing = Timing::start("timeline.apply_retention");
    retain(&mut history, retention_days, now);
    drop(timing);
    if cancelled() {
        return Err("Timeline cancelled; completed scan remains available.".into());
    }
    save_cancellable(directory, &history, cancelled)
}

pub fn record_deletion(
    directory: &Path,
    volume_root: &str,
    scan_root: &str,
    bytes: u64,
    item_count: u64,
) -> Result<(), String> {
    if bytes == 0 || item_count == 0 {
        return Ok(());
    }
    let (mut history, _, _) = load(directory);
    history.schema_version = SCHEMA_VERSION;
    history.deletions.push(DeletionRecord {
        timestamp: Utc::now().to_rfc3339(),
        scope_id: scope_identity(volume_root, scan_root),
        bytes,
        item_count,
    });
    let retention_days = history.retention_days.clamp(30, 180);
    retain(&mut history, retention_days, Utc::now());
    save(directory, &history).map(|_| ())
}

fn signed_delta(current: u64, previous: u64, warnings: &mut Vec<String>) -> i64 {
    let value = current as i128 - previous as i128;
    if value > i64::MAX as i128 || value < i64::MIN as i128 {
        warnings.push("A byte delta exceeded the display range and was clamped.".into());
    }
    value.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

fn category_changes(
    previous: &StorageSnapshot,
    current: &StorageSnapshot,
    warnings: &mut Vec<String>,
) -> Vec<TimelineDelta> {
    let before: HashMap<_, _> = previous
        .categories
        .iter()
        .map(|item| (item.name.as_str(), item.bytes))
        .collect();
    let after: HashMap<_, _> = current
        .categories
        .iter()
        .map(|item| (item.name.as_str(), item.bytes))
        .collect();
    let names: HashSet<_> = before.keys().chain(after.keys()).copied().collect();
    let mut changes: Vec<_> = names
        .into_iter()
        .filter_map(|name| {
            let delta = signed_delta(
                *after.get(name).unwrap_or(&0),
                *before.get(name).unwrap_or(&0),
                warnings,
            );
            (delta != 0).then(|| TimelineDelta {
                name: name.into(),
                path: None,
                delta_bytes: delta,
            })
        })
        .collect();
    changes.sort_by_key(|item| std::cmp::Reverse(item.delta_bytes.unsigned_abs()));
    changes
}

fn path_depth(path: &str) -> usize {
    Path::new(path).components().count()
}

fn developer_category_changes(
    previous: &StorageSnapshot,
    current: &StorageSnapshot,
    warnings: &mut Vec<String>,
) -> Vec<TimelineDelta> {
    let before: HashMap<_, _> = previous
        .developer_categories
        .iter()
        .map(|item| (item.name.as_str(), item.bytes))
        .collect();
    let after: HashMap<_, _> = current
        .developer_categories
        .iter()
        .map(|item| (item.name.as_str(), item.bytes))
        .collect();
    let names: HashSet<_> = before.keys().chain(after.keys()).copied().collect();
    let mut changes: Vec<_> = names
        .into_iter()
        .filter_map(|name| {
            let delta = signed_delta(
                *after.get(name).unwrap_or(&0),
                *before.get(name).unwrap_or(&0),
                warnings,
            );
            (delta != 0).then(|| TimelineDelta {
                name: name.into(),
                path: None,
                delta_bytes: delta,
            })
        })
        .collect();
    changes.sort_by_key(|item| std::cmp::Reverse(item.delta_bytes.unsigned_abs()));
    changes
}

fn directory_changes(
    previous: &StorageSnapshot,
    current: &StorageSnapshot,
    warnings: &mut Vec<String>,
) -> Vec<TimelineDelta> {
    let before: HashMap<_, _> = previous
        .directories
        .iter()
        .map(|item| (normalize_path(&item.path), item))
        .collect();
    let after: HashMap<_, _> = current
        .directories
        .iter()
        .map(|item| (normalize_path(&item.path), item))
        .collect();
    let paths: HashSet<_> = before.keys().chain(after.keys()).cloned().collect();
    let threshold = 100 * 1024 * 1024u64;
    let mut candidates: Vec<_> = paths
        .into_iter()
        .filter_map(|path| {
            let delta = signed_delta(
                after.get(&path).map_or(0, |item| item.bytes),
                before.get(&path).map_or(0, |item| item.bytes),
                warnings,
            );
            let display_path = after
                .get(&path)
                .or_else(|| before.get(&path))
                .map(|item| item.path.clone())
                .unwrap_or(path);
            (delta.unsigned_abs() >= threshold).then(|| TimelineDelta {
                name: Path::new(&display_path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path: Some(display_path),
                delta_bytes: delta,
            })
        })
        .collect();
    candidates.sort_by(|left, right| {
        path_depth(right.path.as_deref().unwrap_or_default())
            .cmp(&path_depth(left.path.as_deref().unwrap_or_default()))
            .then_with(|| {
                right
                    .delta_bytes
                    .unsigned_abs()
                    .cmp(&left.delta_bytes.unsigned_abs())
            })
    });
    let mut attributed: Vec<TimelineDelta> = vec![];
    for candidate in candidates {
        let path = candidate.path.as_deref().unwrap_or_default();
        let descendant_total: i128 = attributed
            .iter()
            .filter(|item| {
                item.delta_bytes.signum() == candidate.delta_bytes.signum()
                    && item
                        .path
                        .as_deref()
                        .is_some_and(|child| Path::new(child).starts_with(Path::new(path)))
            })
            .map(|item| item.delta_bytes as i128)
            .sum();
        if descendant_total.unsigned_abs().saturating_mul(100)
            >= (candidate.delta_bytes.unsigned_abs() as u128).saturating_mul(80)
        {
            continue;
        }
        attributed.push(candidate);
    }
    attributed.sort_by_key(|item| std::cmp::Reverse(item.delta_bytes.unsigned_abs()));
    attributed.truncate(12);
    attributed
}

fn file_changes(
    previous: &StorageSnapshot,
    current: &StorageSnapshot,
    warnings: &mut Vec<String>,
) -> Vec<TimelineFileChange> {
    let before: HashMap<_, _> = previous
        .large_files
        .iter()
        .map(|file| (normalize_path(&file.path), file))
        .collect();
    let after: HashMap<_, _> = current
        .large_files
        .iter()
        .map(|file| (normalize_path(&file.path), file))
        .collect();
    let paths: HashSet<_> = before.keys().chain(after.keys()).cloned().collect();
    let mut changes: Vec<_> = paths
        .into_iter()
        .filter_map(|path| match (before.get(&path), after.get(&path)) {
            (None, Some(file)) => Some(TimelineFileChange {
                path: file.path.clone(),
                name: file.name.clone(),
                kind: "NEW".into(),
                previous_size: None,
                current_size: Some(file.size),
                delta_bytes: signed_delta(file.size, 0, warnings),
            }),
            (Some(file), None) => Some(TimelineFileChange {
                path: file.path.clone(),
                name: file.name.clone(),
                kind: "REMOVED".into(),
                previous_size: Some(file.size),
                current_size: None,
                delta_bytes: signed_delta(0, file.size, warnings),
            }),
            (Some(old), Some(new)) if old.size != new.size => Some(TimelineFileChange {
                path: new.path.clone(),
                name: new.name.clone(),
                kind: if new.size > old.size {
                    "GROWN"
                } else {
                    "SHRUNK"
                }
                .into(),
                previous_size: Some(old.size),
                current_size: Some(new.size),
                delta_bytes: signed_delta(new.size, old.size, warnings),
            }),
            _ => None,
        })
        .collect();
    changes.sort_by_key(|item| std::cmp::Reverse(item.delta_bytes.unsigned_abs()));
    changes.truncate(30);
    changes
}

fn compare(
    previous: &StorageSnapshot,
    current: &StorageSnapshot,
    deletions: &[DeletionRecord],
) -> Result<StorageChangeReport, String> {
    if previous.scope_id != current.scope_id {
        return Err("Timeline snapshots belong to incompatible scan scopes.".into());
    }
    let mut warnings = vec![];
    let category_deltas = category_changes(previous, current, &mut warnings);
    let developer_category_deltas = developer_category_changes(previous, current, &mut warnings);
    let directory_deltas = directory_changes(previous, current, &mut warnings);
    let file_changes = file_changes(previous, current, &mut warnings);
    let recovered = deletions
        .iter()
        .filter(|record| {
            record.scope_id == current.scope_id
                && record.timestamp > previous.timestamp
                && record.timestamp <= current.timestamp
        })
        .fold(0u64, |total, record| total.saturating_add(record.bytes));
    let used_delta = signed_delta(current.used_bytes, previous.used_bytes, &mut warnings);
    let direction = if used_delta > 0 {
        "increased"
    } else if used_delta < 0 {
        "decreased"
    } else {
        "did not change"
    };
    let strongest = directory_deltas
        .first()
        .map(|item| format!(" The largest observed directory change was {}.", item.name))
        .unwrap_or_default();
    let largest_file = file_changes
        .iter()
        .find(|item| item.kind == "NEW")
        .map(|item| format!(" The largest newly detected file was {}.", item.name))
        .unwrap_or_default();
    Ok(StorageChangeReport {
        previous_timestamp: previous.timestamp.clone(),
        current_timestamp: current.timestamp.clone(),
        used_space_delta: used_delta,
        free_space_delta: signed_delta(current.free_bytes, previous.free_bytes, &mut warnings),
        scanned_size_delta: signed_delta(
            current.scanned_bytes,
            previous.scanned_bytes,
            &mut warnings,
        ),
        category_deltas,
        developer_category_deltas,
        directory_deltas,
        file_changes,
        spacepilot_recovered_bytes: recovered,
        summary: format!(
            "Disk usage {direction} between these two real snapshots.{strongest}{largest_file}"
        ),
        warnings,
    })
}

pub fn response(directory: &Path, active_root: Option<&str>, range_days: u32) -> TimelineResponse {
    let (history, mut warnings, history_bytes) = load(directory);
    let scope = match active_root {
        Some(root) => {
            let root = normalize_path(root);
            history
                .snapshots
                .iter()
                .rev()
                .find(|snapshot| normalize_path(&snapshot.scan_root) == root)
                .map(|snapshot| snapshot.scope_id.clone())
        }
        None => history
            .snapshots
            .last()
            .map(|snapshot| snapshot.scope_id.clone()),
    };
    let Some(scope) = scope else {
        return TimelineResponse {
            snapshots: vec![],
            report: None,
            first_snapshot: false,
            insufficient_range: false,
            scope_root: None,
            warnings,
            history_bytes,
        };
    };
    let mut compatible: Vec<_> = history
        .snapshots
        .iter()
        .filter(|snapshot| snapshot.scope_id == scope)
        .collect();
    compatible.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let point_cutoff = compatible.last().and_then(|snapshot| {
        DateTime::parse_from_rfc3339(&snapshot.timestamp)
            .ok()
            .map(|timestamp| timestamp - Duration::days(range_days.clamp(1, 3650) as i64))
    });
    let points = compatible
        .iter()
        .filter(|snapshot| {
            point_cutoff.is_none_or(|cutoff| {
                DateTime::parse_from_rfc3339(&snapshot.timestamp)
                    .is_ok_and(|timestamp| timestamp >= cutoff)
            })
        })
        .map(|snapshot| TimelinePoint {
            timestamp: snapshot.timestamp.clone(),
            used_bytes: snapshot.used_bytes,
            free_bytes: snapshot.free_bytes,
            scanned_bytes: snapshot.scanned_bytes,
        })
        .collect();
    let report = compatible.last().and_then(|current| {
        let current_time = DateTime::parse_from_rfc3339(&current.timestamp).ok()?;
        let cutoff = current_time - Duration::days(range_days.clamp(1, 3650) as i64);
        let previous = compatible
            .iter()
            .take(compatible.len().saturating_sub(1))
            .find(|snapshot| {
                DateTime::parse_from_rfc3339(&snapshot.timestamp)
                    .is_ok_and(|timestamp| timestamp >= cutoff)
            });
        previous.and_then(
            |previous| match compare(previous, current, &history.deletions) {
                Ok(report) => Some(report),
                Err(error) => {
                    warnings.push(error);
                    None
                }
            },
        )
    });
    let insufficient_range = compatible.len() > 1 && report.is_none();
    TimelineResponse {
        snapshots: points,
        report,
        first_snapshot: compatible.len() == 1,
        insufficient_range,
        scope_root: compatible.last().map(|snapshot| snapshot.scan_root.clone()),
        warnings,
        history_bytes,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancellation_preserves_existing_history_and_completed_scan() {
        let location = std::env::temp_dir().join(format!(
            "spacepilot-timeline-cancel-{}",
            uuid::Uuid::new_v4()
        ));
        let summary = summary("C:\\scan");
        record_snapshot(&location, &summary, &HashMap::new(), &[], 90).unwrap();
        let original = fs::read(history_path(&location)).unwrap();
        let result =
            record_snapshot_cancellable(&location, &summary, &HashMap::new(), &[], 90, &|| true);
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(!summary.cancelled);
        assert_eq!(fs::read(history_path(&location)).unwrap(), original);
        let result = save_cancellable(&location, &TimelineHistory::default(), &|| true);
        assert!(result.is_err());
        assert_eq!(fs::read(history_path(&location)).unwrap(), original);
        assert_eq!(fs::read_dir(&location).unwrap().count(), 1);
        fs::remove_dir_all(location).unwrap();
    }

    #[test]
    fn corrupt_history_is_preserved_on_snapshot_attempt() {
        let location = std::env::temp_dir().join(format!(
            "spacepilot-timeline-corrupt-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&location).unwrap();
        fs::write(history_path(&location), b"invalid history").unwrap();
        assert!(
            record_snapshot(&location, &summary("C:\\scan"), &HashMap::new(), &[], 90).is_err()
        );
        assert_eq!(
            fs::read(history_path(&location)).unwrap(),
            b"invalid history"
        );
        fs::remove_dir_all(location).unwrap();
    }
    #[test]
    #[ignore = "explicit large in-memory performance measurement"]
    fn synthetic_550k_timeline() {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            let (history, warnings, bytes) =
                load(&PathBuf::from(app_data).join("site.hatchable.spacepilot"));
            eprintln!(
                "existing_history bytes={bytes} snapshots={} deletions={} warnings={}",
                history.snapshots.len(),
                history.deletions.len(),
                warnings.len()
            );
        }
        let mut files = Vec::with_capacity(550_000);
        let mut directories = HashMap::new();
        for index in 0..70_000 {
            let path = format!("C:\\scan\\project{index}\\node_modules");
            directories.insert(
                path.clone(),
                FileEntry {
                    path,
                    name: "node_modules".into(),
                    extension: String::new(),
                    size: 8 * 150 * 1024 * 1024,
                    modified: None,
                    category: "Other".into(),
                    is_directory: true,
                },
            );
        }
        for index in 0..550_000 {
            files.push(FileEntry {
                path: format!(
                    "C:\\scan\\project{}\\node_modules\\file{index}.bin",
                    index % 70_000
                ),
                name: format!("file{index}.bin"),
                extension: "bin".into(),
                size: 150 * 1024 * 1024,
                modified: Some("2026-09-16T00:00:00Z".into()),
                category: "Other".into(),
                is_directory: false,
            });
        }
        let location = std::env::temp_dir().join(format!(
            "spacepilot-timeline-bench-{}",
            uuid::Uuid::new_v4()
        ));
        let started = std::time::Instant::now();
        let result = snapshot(&summary("C:\\scan"), &directories, &files, Utc::now());
        let build = started.elapsed();
        assert_eq!(result.directories.len(), MAX_DIRECTORIES);
        assert_eq!(result.large_files.len(), MAX_LARGE_FILES);
        let bytes = serde_json::to_vec(&result).unwrap().len();
        eprintln!(
            "benchmark files={} directories={} build_ms={:.3} snapshot_bytes={bytes}",
            files.len(),
            directories.len(),
            build.as_secs_f64() * 1000.0
        );
        let history = TimelineHistory {
            snapshots: vec![result],
            ..TimelineHistory::default()
        };
        save(&location, &history).unwrap();
        record_snapshot(&location, &summary("C:\\scan"), &directories, &files, 90).unwrap();
        std::fs::remove_dir_all(location).unwrap();
    }
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spacepilot-timeline-{name}-{nonce}"))
    }
    fn sample(root: &str, at: DateTime<Utc>, scanned: u64) -> StorageSnapshot {
        StorageSnapshot {
            schema_version: 1,
            timestamp: at.to_rfc3339(),
            scope_id: scope_identity("C:\\", root),
            volume_root: "C:\\".into(),
            volume_capacity: 1_000,
            used_bytes: scanned,
            free_bytes: 1_000u64.saturating_sub(scanned),
            scan_root: root.into(),
            scanned_bytes: scanned,
            file_count: 1,
            directory_count: 1,
            categories: vec![SnapshotCategory {
                name: "Videos".into(),
                bytes: scanned,
            }],
            developer_categories: vec![],
            directories: vec![],
            large_files: vec![],
        }
    }

    fn summary(root: &str) -> ScanSummary {
        ScanSummary {
            root: root.into(),
            total_size: 1_000,
            volume_root: "C:\\".into(),
            volume_capacity: 10_000,
            volume_used: 6_000,
            volume_free: 4_000,
            file_count: 1,
            directory_count: 1,
            category_totals: vec![CategoryTotal {
                category: "Other".into(),
                size: 1_000,
                files: 1,
            }],
            largest_files: vec![],
            largest_directories: vec![],
            scanned_at: Utc::now().to_rfc3339(),
            cancelled: false,
            inaccessible_count: 0,
            timeline_warning: None,
        }
    }

    #[test]
    fn completed_scan_creates_and_persists_a_snapshot() {
        let directory = temp("snapshot");
        let stored =
            record_snapshot(&directory, &summary("C:\\scan"), &HashMap::new(), &[], 90).unwrap();
        assert!(stored > 0);
        let view = response(&directory, Some("C:\\scan"), 7);
        assert!(view.first_snapshot);
        assert_eq!(view.snapshots.len(), 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn maximum_snapshot_metadata_remains_compact() {
        let root = "C:\\scan";
        let mut directories = HashMap::new();
        for index in 0..MAX_DIRECTORIES {
            let path = format!("{root}\\directory-{index}");
            directories.insert(
                path.clone(),
                FileEntry {
                    path,
                    name: format!("directory-{index}"),
                    extension: String::new(),
                    size: 1_000_000_000,
                    modified: None,
                    category: "Other".into(),
                    is_directory: true,
                },
            );
        }
        let files: Vec<_> = (0..MAX_LARGE_FILES)
            .map(|index| FileEntry {
                path: format!("{root}\\large-file-{index}.bin"),
                name: format!("large-file-{index}.bin"),
                extension: "bin".into(),
                size: LARGE_FILE_THRESHOLD,
                modified: Some("2026-01-01T00:00:00+00:00".into()),
                category: "Other".into(),
                is_directory: false,
            })
            .collect();
        let encoded =
            serde_json::to_vec(&snapshot(&summary(root), &directories, &files, Utc::now()))
                .unwrap();
        println!("maximum synthetic snapshot bytes: {}", encoded.len());
        assert!(encoded.len() < 1024 * 1024);
    }

    #[test]
    fn scope_identity_is_case_insensitive_on_windows() {
        if cfg!(windows) {
            assert_eq!(
                scope_identity("C:\\", "C:\\Users"),
                scope_identity("c:/", "c:/users/")
            );
        }
        assert_ne!(
            scope_identity("C:\\", "C:\\Users"),
            scope_identity("D:\\", "D:\\Users")
        );
    }

    #[test]
    fn compares_positive_negative_and_category_deltas() {
        let now = Utc::now();
        let before = sample("C:\\scan", now - Duration::days(1), 100);
        let after = sample("C:\\scan", now, 250);
        let report = compare(&before, &after, &[]).unwrap();
        assert_eq!(report.used_space_delta, 150);
        assert_eq!(report.free_space_delta, -150);
        assert_eq!(report.category_deltas[0].delta_bytes, 150);
        assert_eq!(
            compare(&after, &before, &[]).unwrap().used_space_delta,
            -150
        );
    }

    #[test]
    fn incompatible_scopes_are_rejected() {
        let now = Utc::now();
        assert!(compare(&sample("C:\\a", now, 1), &sample("C:\\b", now, 1), &[]).is_err());
    }

    #[test]
    fn parent_child_growth_is_attributed_once() {
        let now = Utc::now();
        let mut before = sample("C:\\scan", now - Duration::days(1), 0);
        let mut after = sample("C:\\scan", now, 600_000_000);
        for snapshot in [&mut before, &mut after] {
            snapshot.directories = vec![
                SnapshotDirectory {
                    path: "C:\\scan\\parent".into(),
                    bytes: 0,
                },
                SnapshotDirectory {
                    path: "C:\\scan\\parent\\child".into(),
                    bytes: 0,
                },
            ];
        }
        after.directories[0].bytes = 500_000_000;
        after.directories[1].bytes = 500_000_000;
        let changes = compare(&before, &after, &[]).unwrap().directory_deltas;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "child");
    }

    #[test]
    fn detects_new_removed_and_grown_large_files() {
        let now = Utc::now();
        let mut before = sample("C:\\scan", now - Duration::days(1), 0);
        let mut after = sample("C:\\scan", now, 0);
        before.large_files = vec![
            SnapshotFile {
                path: "C:\\scan\\gone.bin".into(),
                name: "gone.bin".into(),
                size: 200,
                modified: None,
                category: "Other".into(),
            },
            SnapshotFile {
                path: "C:\\scan\\grown.bin".into(),
                name: "grown.bin".into(),
                size: 200,
                modified: None,
                category: "Other".into(),
            },
        ];
        after.large_files = vec![
            SnapshotFile {
                path: "C:\\scan\\new.bin".into(),
                name: "new.bin".into(),
                size: 300,
                modified: None,
                category: "Other".into(),
            },
            SnapshotFile {
                path: "C:\\scan\\grown.bin".into(),
                name: "grown.bin".into(),
                size: 500,
                modified: None,
                category: "Other".into(),
            },
        ];
        let kinds: HashSet<_> = compare(&before, &after, &[])
            .unwrap()
            .file_changes
            .into_iter()
            .map(|item| item.kind)
            .collect();
        assert_eq!(
            kinds,
            HashSet::from(["NEW".into(), "REMOVED".into(), "GROWN".into()])
        );
    }

    #[test]
    fn only_recorded_spacepilot_deletions_are_attributed() {
        let now = Utc::now();
        let before = sample("C:\\scan", now - Duration::days(2), 800);
        let after = sample("C:\\scan", now, 200);
        assert_eq!(
            compare(&before, &after, &[])
                .unwrap()
                .spacepilot_recovered_bytes,
            0
        );
        let deletion = DeletionRecord {
            timestamp: (now - Duration::days(1)).to_rfc3339(),
            scope_id: after.scope_id.clone(),
            bytes: 300,
            item_count: 1,
        };
        assert_eq!(
            compare(&before, &after, &[deletion])
                .unwrap()
                .spacepilot_recovered_bytes,
            300
        );
    }

    #[test]
    fn first_snapshot_and_missing_history_are_safe() {
        let directory = temp("first");
        assert!(!response(&directory, None, 7).first_snapshot);
        let now = Utc::now();
        save(
            &directory,
            &TimelineHistory {
                schema_version: 1,
                retention_days: 90,
                snapshots: vec![sample("C:\\scan", now, 1)],
                deletions: vec![],
            },
        )
        .unwrap();
        let view = response(&directory, None, 7);
        assert!(view.first_snapshot);
        assert!(view.report.is_none());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn narrow_range_does_not_compare_an_out_of_range_snapshot() {
        let directory = temp("range");
        let now = Utc::now();
        save(
            &directory,
            &TimelineHistory {
                schema_version: 1,
                retention_days: 90,
                snapshots: vec![
                    sample("C:\\scan", now - Duration::days(10), 1),
                    sample("C:\\scan", now, 2),
                ],
                deletions: vec![],
            },
        )
        .unwrap();
        let view = response(&directory, Some("C:\\scan"), 1);
        assert!(view.insufficient_range);
        assert!(view.report.is_none());
        assert_eq!(view.snapshots.len(), 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn active_scope_never_falls_back_to_an_incompatible_history() {
        let directory = temp("active-scope");
        save(
            &directory,
            &TimelineHistory {
                schema_version: 1,
                retention_days: 90,
                snapshots: vec![sample("D:\\photos", Utc::now(), 2)],
                deletions: vec![],
            },
        )
        .unwrap();
        let view = response(&directory, Some("C:\\downloads"), 30);
        assert!(view.snapshots.is_empty());
        assert!(view.report.is_none());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn corrupted_history_is_ignored_without_panicking() {
        let directory = temp("corrupt");
        fs::create_dir_all(&directory).unwrap();
        fs::write(history_path(&directory), b"not-json").unwrap();
        let view = response(&directory, None, 7);
        assert!(view.report.is_none());
        assert_eq!(view.warnings.len(), 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn unsupported_schema_is_ignored_for_safe_upgrades() {
        let directory = temp("schema");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            history_path(&directory),
            br#"{"schema_version":999,"retention_days":90,"snapshots":[],"deletions":[]}"#,
        )
        .unwrap();
        let view = response(&directory, None, 7);
        assert!(view.snapshots.is_empty());
        assert_eq!(view.warnings.len(), 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn retention_expires_old_records_and_bounds_count() {
        let now = DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut history = TimelineHistory::default();
        history
            .snapshots
            .push(sample("C:\\scan", now - Duration::days(31), 1));
        for minute in 0..200 {
            history
                .snapshots
                .push(sample("C:\\scan", now - Duration::minutes(minute), 1));
        }
        retain(&mut history, 30, now);
        assert_eq!(history.snapshots.len(), 4);
        assert!(history
            .snapshots
            .iter()
            .all(|item| item.timestamp > (now - Duration::days(31)).to_rfc3339()));
    }

    #[test]
    fn zero_change_and_extreme_deltas_are_safe() {
        let now = Utc::now();
        let same = sample("C:\\scan", now, 10);
        assert_eq!(compare(&same, &same, &[]).unwrap().used_space_delta, 0);
        let mut before = same.clone();
        let mut after = same;
        before.used_bytes = 0;
        after.used_bytes = u64::MAX;
        let report = compare(&before, &after, &[]).unwrap();
        assert_eq!(report.used_space_delta, i64::MAX);
        assert!(!report.warnings.is_empty());
    }
}
