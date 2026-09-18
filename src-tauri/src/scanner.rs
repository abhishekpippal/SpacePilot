use crate::{categories, models::*};
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    fs::Metadata,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}
#[derive(Clone)]
pub struct ScanState {
    pub files: Arc<Mutex<Vec<FileEntry>>>,
    pub directories: Arc<Mutex<HashMap<String, FileEntry>>>,
    pub root: Arc<Mutex<Option<String>>>,
    pub cancelled: Arc<AtomicBool>,
    pub duplicate_cancelled: Arc<AtomicBool>,
    pub operation_running: Arc<AtomicBool>,
    pub duplicate_groups: Arc<Mutex<Vec<DuplicateGroup>>>,
    pub duplicate_analysis_complete: Arc<AtomicBool>,
    pub last_summary: Arc<Mutex<Option<ScanSummary>>>,
    pub ai_cancelled: Arc<AtomicBool>,
    pub progress: Arc<Mutex<ScanProgress>>,
}
impl Default for ScanState {
    fn default() -> Self {
        Self {
            files: Arc::new(Mutex::new(vec![])),
            directories: Arc::new(Mutex::new(HashMap::new())),
            root: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
            duplicate_cancelled: Arc::new(AtomicBool::new(false)),
            operation_running: Arc::new(AtomicBool::new(false)),
            duplicate_groups: Arc::new(Mutex::new(vec![])),
            duplicate_analysis_complete: Arc::new(AtomicBool::new(false)),
            last_summary: Arc::new(Mutex::new(None)),
            ai_cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(ScanProgress::default())),
        }
    }
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn report_progress<F>(
    state: &ScanState,
    on_progress: &mut F,
    phase: &str,
    files: u64,
    directories: u64,
    bytes: u64,
    inaccessible: u64,
    path: String,
    activity: &str,
) where
    F: FnMut(ScanProgress),
{
    let progress = ScanProgress {
        phase: phase.into(),
        files_scanned: files,
        directories_scanned: directories,
        bytes_scanned: bytes,
        inaccessible_count: inaccessible,
        current_path: path,
        last_progress_unix_ms: unix_millis(),
        activity: activity.into(),
        stalled: false,
    };
    if let Ok(mut current) = state.progress.lock() {
        *current = progress.clone();
    }
    on_progress(progress);
}
#[cfg(test)]
pub fn aggregate_directory_sizes(root: &Path, files: &[FileEntry]) -> HashMap<String, u64> {
    aggregate_directory_sizes_cancellable(root, files, || false).0
}

fn aggregate_directory_sizes_cancellable<C>(
    root: &Path,
    files: &[FileEntry],
    is_cancelled: C,
) -> (HashMap<String, u64>, bool)
where
    C: Fn() -> bool,
{
    let mut sizes = HashMap::new();
    sizes.insert(root.to_string_lossy().to_string(), 0u64);
    for (index, file) in files.iter().enumerate() {
        if index % 256 == 0 && is_cancelled() {
            return (sizes, true);
        }
        let mut ancestor = Path::new(&file.path).parent();
        while let Some(directory) = ancestor {
            if !directory.starts_with(root) {
                break;
            }
            let value = sizes
                .entry(directory.to_string_lossy().to_string())
                .or_insert(0);
            *value = value.saturating_add(file.size);
            if directory == root {
                break;
            }
            ancestor = directory.parent();
        }
    }
    (sizes, false)
}
fn volume_statistics(path: &Path) -> Result<(String, u64, u64, u64), String> {
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    let volume_root = canonical
        .ancestors()
        .last()
        .unwrap_or(canonical.as_path())
        .to_string_lossy()
        .to_string();
    let capacity = fs2::total_space(&canonical).map_err(|error| error.to_string())?;
    let free = fs2::available_space(&canonical).map_err(|error| error.to_string())?;
    Ok((volume_root, capacity, used_space(capacity, free), free))
}
fn used_space(capacity: u64, free: u64) -> u64 {
    capacity.saturating_sub(free)
}
fn scan_with_progress<F>(
    state: &ScanState,
    root: String,
    mut on_progress: F,
) -> Result<ScanSummary, String>
where
    F: FnMut(ScanProgress),
{
    let canonical_root = Path::new(&root)
        .canonicalize()
        .map_err(|error| format!("The selected location is unavailable: {error}"))?;
    let root_path = canonical_root.as_path();
    if !root_path.is_dir() {
        return Err("Select an accessible folder or volume.".into());
    }
    let root = root_path.to_string_lossy().to_string();
    state.cancelled.store(false, Ordering::Relaxed);
    *state.root.lock().map_err(|_| "Scan state unavailable")? = Some(root.clone());
    state
        .duplicate_groups
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clear();
    state
        .duplicate_analysis_complete
        .store(false, Ordering::Release);
    let mut files = Vec::new();
    let mut directory_entries: HashMap<String, FileEntry> = HashMap::new();
    let mut totals: HashMap<String, (u64, u64)> = HashMap::new();
    let (mut byte_count, mut file_count, mut dir_count, mut inaccessible): (u64, u64, u64, u64) =
        (0, 0, 0, 0);
    report_progress(
        state,
        &mut on_progress,
        "DISCOVERING",
        0,
        0,
        0,
        0,
        root.clone(),
        "Starting directory traversal",
    );
    let walker = WalkDir::new(root_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || entry.path().symlink_metadata().is_ok_and(|metadata| {
                    !entry.file_type().is_symlink() && !is_reparse_point(&metadata)
                })
        });
    for item in walker {
        if state.cancelled.load(Ordering::Relaxed) {
            break;
        }
        let entry = match item {
            Ok(v) => v,
            Err(_) => {
                inaccessible += 1;
                continue;
            }
        };
        let link_metadata = match entry.path().symlink_metadata() {
            Ok(value) => value,
            Err(_) => {
                inaccessible += 1;
                continue;
            }
        };
        // Reparse points, junctions and symbolic links are not traversed or recorded.
        // This keeps the scan and later deletion boundary on one physical tree.
        if entry.file_type().is_symlink() || is_reparse_point(&link_metadata) {
            inaccessible += 1;
            continue;
        }
        let meta = match entry.metadata() {
            Ok(v) => v,
            Err(_) => {
                inaccessible += 1;
                continue;
            }
        };
        if meta.is_dir() {
            dir_count += 1;
            let path = entry.path();
            directory_entries.insert(
                path.to_string_lossy().to_string(),
                FileEntry {
                    path: path.to_string_lossy().to_string(),
                    name: entry.file_name().to_string_lossy().to_string(),
                    extension: String::new(),
                    size: 0,
                    modified: meta
                        .modified()
                        .ok()
                        .map(|time| DateTime::<Utc>::from(time).to_rfc3339()),
                    category: "Other".into(),
                    is_directory: true,
                },
            );
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let extension = path
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_string();
        let category = categories::category(&extension);
        let modified = meta
            .modified()
            .ok()
            .map(|t| DateTime::<Utc>::from(t).to_rfc3339());
        let record = FileEntry {
            path: path.to_string_lossy().to_string(),
            name,
            extension,
            size,
            modified,
            category: category.clone(),
            is_directory: false,
        };
        let t = totals.entry(category).or_insert((0, 0));
        t.0 = t.0.saturating_add(size);
        t.1 = t.1.saturating_add(1);
        byte_count = byte_count.saturating_add(size);
        file_count = file_count.saturating_add(1);
        if file_count % 128 == 0 {
            report_progress(
                state,
                &mut on_progress,
                "SCANNING_METADATA",
                file_count,
                dir_count,
                byte_count,
                inaccessible,
                record.path.clone(),
                "Reading filesystem metadata",
            );
        }
        files.push(record);
    }
    let mut cancelled = state.cancelled.load(Ordering::Relaxed);
    if !cancelled {
        report_progress(
            state,
            &mut on_progress,
            "ANALYZING",
            file_count,
            dir_count,
            byte_count,
            inaccessible,
            String::new(),
            "Ranking files and aggregating directories",
        );
        if state.cancelled.load(Ordering::Relaxed) {
            cancelled = true;
        } else {
            files.sort_by(|a, b| b.size.cmp(&a.size));
            let (directory_sizes, aggregation_cancelled) =
                aggregate_directory_sizes_cancellable(root_path, &files, || {
                    state.cancelled.load(Ordering::Relaxed)
                });
            cancelled = aggregation_cancelled;
            if !cancelled {
                for (path, size) in directory_sizes {
                    if let Some(directory) = directory_entries.get_mut(&path) {
                        directory.size = size;
                    }
                }
            }
        }
    }
    let mut directories: Vec<FileEntry> = directory_entries.values().cloned().collect();
    if !cancelled {
        directories.sort_by(|a, b| b.size.cmp(&a.size));
    }
    let categories = totals
        .into_iter()
        .map(|(category, (size, files))| CategoryTotal {
            category,
            size,
            files,
        })
        .collect();
    let largest_files = if cancelled {
        vec![]
    } else {
        files.iter().take(100).cloned().collect()
    };
    let largest_directories = if cancelled {
        vec![]
    } else {
        directories.into_iter().take(100).collect()
    };
    *state.files.lock().map_err(|_| "Scan state unavailable")? = files;
    *state
        .directories
        .lock()
        .map_err(|_| "Scan state unavailable")? = directory_entries;
    report_progress(
        state,
        &mut on_progress,
        "FINALIZING",
        file_count,
        dir_count,
        byte_count,
        inaccessible,
        String::new(),
        "Reading volume statistics and preparing results",
    );
    let (volume_root, volume_capacity, volume_used, volume_free) = volume_statistics(root_path)?;
    Ok(ScanSummary {
        root,
        total_size: byte_count,
        volume_root,
        volume_capacity,
        volume_used,
        volume_free,
        file_count,
        directory_count: dir_count,
        category_totals: categories,
        largest_files,
        largest_directories,
        scanned_at: Utc::now().to_rfc3339(),
        cancelled,
        inaccessible_count: inaccessible,
        timeline_warning: None,
    })
}

pub fn scan(app: &AppHandle, state: &ScanState, root: String) -> Result<ScanSummary, String> {
    scan_with_progress(state, root, |progress| {
        let _ = app.emit("scan-progress", progress);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn fixture(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spacepilot-scan-{name}-{nonce}"))
    }
    fn file(path: &str, size: u64) -> FileEntry {
        FileEntry {
            path: path.into(),
            name: "file".into(),
            extension: "bin".into(),
            size,
            modified: None,
            category: "Other".into(),
            is_directory: false,
        }
    }
    #[test]
    fn aggregates_descendants_without_double_counting() {
        let root = Path::new("C:\\scan");
        let sizes = aggregate_directory_sizes(
            root,
            &[
                file("C:\\scan\\a\\one.bin", 10),
                file("C:\\scan\\a\\b\\two.bin", 20),
            ],
        );
        assert_eq!(sizes["C:\\scan"], 30);
        assert_eq!(sizes["C:\\scan\\a"], 30);
        assert_eq!(sizes["C:\\scan\\a\\b"], 20);
    }
    #[test]
    fn used_space_never_underflows() {
        assert_eq!(used_space(1_000, 250), 750);
        assert_eq!(used_space(100, 200), 0);
    }

    #[test]
    fn aggregation_ignores_files_outside_scope_and_handles_empty_input() {
        let root = Path::new("C:\\scan");
        let sizes = aggregate_directory_sizes(root, &[file("D:\\outside\\file.bin", 50)]);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes["C:\\scan"], 0);
        assert_eq!(aggregate_directory_sizes(root, &[])["C:\\scan"], 0);
    }

    #[test]
    fn aggregates_large_deep_file_sets() {
        let root = Path::new("C:\\scan");
        let files: Vec<_> = (0..100_000)
            .map(|index| file(&format!("C:\\scan\\a\\b\\{index}.bin"), 7))
            .collect();
        let sizes = aggregate_directory_sizes(root, &files);
        assert_eq!(sizes["C:\\scan"], 700_000);
        assert_eq!(sizes["C:\\scan\\a\\b"], 700_000);
    }

    #[test]
    fn scanner_cancels_and_keeps_partial_results_consistent() {
        let base = fixture("cancel");
        fs::create_dir_all(&base).unwrap();
        for index in 0..300 {
            fs::write(base.join(format!("{index}.bin")), b"x").unwrap();
        }
        let state = ScanState::default();
        let cancellation = state.cancelled.clone();
        let summary = scan_with_progress(&state, base.to_string_lossy().into_owned(), |progress| {
            if progress.files_scanned >= 128 {
                cancellation.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert!(summary.cancelled);
        assert!(summary.file_count >= 128 && summary.file_count < 300);
        assert_eq!(state.files.lock().unwrap().len() as u64, summary.file_count);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn scanner_rejects_missing_roots_without_panicking() {
        let state = ScanState::default();
        let result = scan_with_progress(
            &state,
            fixture("missing").to_string_lossy().into_owned(),
            |_| {},
        );
        assert!(result.is_err());
    }

    #[test]
    fn progress_is_monotonic_and_exposes_real_scan_phases() {
        let base = fixture("progress-phases");
        fs::create_dir_all(base.join("nested")).unwrap();
        for index in 0..260 {
            fs::write(
                base.join("nested").join(format!("file-{index}.txt")),
                b"data",
            )
            .unwrap();
        }
        let state = ScanState::default();
        let mut updates = Vec::new();
        let summary = scan_with_progress(&state, base.to_string_lossy().into_owned(), |progress| {
            updates.push(progress)
        })
        .unwrap();
        assert!(!summary.cancelled);
        assert!(updates.iter().any(|update| update.phase == "DISCOVERING"));
        assert!(updates
            .iter()
            .any(|update| update.phase == "SCANNING_METADATA"));
        assert!(updates.iter().any(|update| update.phase == "ANALYZING"));
        assert_eq!(updates.last().unwrap().phase, "FINALIZING");
        for pair in updates.windows(2) {
            assert!(pair[1].files_scanned >= pair[0].files_scanned);
            assert!(pair[1].directories_scanned >= pair[0].directories_scanned);
            assert!(pair[1].bytes_scanned >= pair[0].bytes_scanned);
            assert!(pair[1].last_progress_unix_ms >= pair[0].last_progress_unix_ms);
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn directory_aggregation_can_be_cancelled_without_partial_sizes_becoming_results() {
        let root = Path::new("C:\\scan");
        let files: Vec<_> = (0..2_000)
            .map(|index| file(&format!("C:\\scan\\deep\\{index}.bin"), 1))
            .collect();
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let (_, cancelled) = aggregate_directory_sizes_cancellable(root, &files, || {
            checks.fetch_add(1, Ordering::Relaxed) >= 2
        });
        assert!(cancelled);
    }
}
