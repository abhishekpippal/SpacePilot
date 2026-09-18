mod ai;
mod categories;
mod cleanup;
mod commercial;
mod developer_storage;
mod duplicates;
mod file_inspector;
mod models;
mod rescue;
mod scanner;
mod timeline;
use scanner::ScanState;
use std::collections::{HashMap, HashSet};
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager, State};

struct AtomicGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl AtomicGuard {
    fn acquire(
        flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        message: &str,
    ) -> Result<Self, String> {
        flag.compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| message.to_string())?;
        Ok(Self(flag))
    }
}

impl Drop for AtomicGuard {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn contains_reparse_point(root: &Path, candidate: &Path) -> Result<bool, String> {
    let mut current = Some(candidate);
    while let Some(path) = current {
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Ok(true);
        }
        if path == root {
            return Ok(false);
        }
        current = path.parent();
    }
    Ok(false)
}

fn modified_string(metadata: &Metadata) -> Option<String> {
    metadata
        .modified()
        .ok()
        .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339())
}

fn validate_delete_candidate(
    root: &Path,
    candidate: &Path,
    scanned: &models::FileEntry,
    application_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    if scanned.is_directory || candidate != Path::new(&scanned.path) {
        return Err(format!(
            "{}: not an explicitly selected scanned file",
            candidate.display()
        ));
    }
    if contains_reparse_point(root, candidate)? {
        return Err(format!(
            "{}: symbolic links, junctions, and reparse points are not eligible",
            candidate.display()
        ));
    }
    let real = candidate
        .canonicalize()
        .map_err(|error| format!("{}: {error}", candidate.display()))?;
    if real == root || !real.starts_with(root) {
        return Err(format!("{}: outside scan location", candidate.display()));
    }
    if categories::protected(&real) {
        return Err(format!(
            "{}: protected system location",
            candidate.display()
        ));
    }
    if application_dir.is_some_and(|directory| real.starts_with(directory)) {
        return Err(format!(
            "{}: SpacePilot application file",
            candidate.display()
        ));
    }
    let metadata = real
        .metadata()
        .map_err(|error| format!("{}: {error}", candidate.display()))?;
    if !metadata.is_file() {
        return Err(format!("{}: not a regular file", candidate.display()));
    }
    if metadata.len() != scanned.size || modified_string(&metadata) != scanned.modified {
        return Err(format!(
            "{}: file changed after scanning; rescan before removing it",
            candidate.display()
        ));
    }
    Ok(real)
}
#[tauri::command]
fn select_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}
#[tauri::command]
fn select_inspector_file() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Supported text files", &["txt", "md", "csv", "json", "log"])
        .add_filter("Documents", &["pdf", "docx"])
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}
#[tauri::command]
async fn scan_folder(
    app: tauri::AppHandle,
    state: State<'_, ScanState>,
    path: String,
    retention_days: u32,
) -> Result<models::ScanSummary, String> {
    let scan_state = state.inner().clone();
    let guard = AtomicGuard::acquire(
        scan_state.operation_running.clone(),
        "Another filesystem operation is already running.",
    )?;
    let timeline_directory = app.path().app_data_dir().ok();
    if let Ok(mut progress) = scan_state.progress.lock() {
        *progress = models::ScanProgress {
            phase: "VALIDATING_SCOPE".into(),
            current_path: path.clone(),
            activity: "Validating the selected scan location".into(),
            last_progress_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            ..models::ScanProgress::default()
        };
        let _ = app.emit("scan-progress", progress.clone());
    }
    let heartbeat_app = app.clone();
    let heartbeat_progress = scan_state.progress.clone();
    let heartbeat_running = scan_state.operation_running.clone();
    std::thread::spawn(move || {
        let mut reported_stall = false;
        while heartbeat_running.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let Ok(mut progress) = heartbeat_progress.lock() else {
                continue;
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            progress.stalled = now.saturating_sub(progress.last_progress_unix_ms) >= 15_000;
            if progress.stalled && !reported_stall {
                #[cfg(debug_assertions)]
                eprintln!(
                    "SpacePilot scan diagnostic: phase={} activity={} no meaningful progress for at least 15 seconds; files={} directories={} skipped={}",
                    progress.phase,
                    progress.activity,
                    progress.files_scanned,
                    progress.directories_scanned,
                    progress.inaccessible_count
                );
                reported_stall = true;
            } else if !progress.stalled {
                reported_stall = false;
            }
            let _ = heartbeat_app.emit("scan-progress", progress.clone());
        }
    });
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let summary = scanner::scan(&app, &scan_state, path)?;
        if !summary.cancelled {
            if let Ok(mut progress) = scan_state.progress.lock() {
                progress.phase = "SAVING_TIMELINE".into();
                progress.activity = "Building compact Timeline metadata and developer categories, then saving history".into();
                progress.current_path.clear();
                progress.last_progress_unix_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                progress.stalled = false;
                let _ = app.emit("scan-progress", progress.clone());
            }
            match timeline_directory {
                Some(directory) => {
                    let timeline_app = app.clone();
                    let timeline_state = scan_state.clone();
                    let timeline_summary = summary.clone();
                    let directories = scan_state
                        .directories
                        .lock()
                        .map_err(|_| "Scan state unavailable")?
                        .clone();
                    let files = scan_state
                        .files
                        .lock()
                        .map_err(|_| "Scan state unavailable")?
                        .clone();
                    let _ = app.emit(
                        "timeline-status",
                        serde_json::json!({
                            "status": "saving",
                            "message": "Saving compact Timeline metadata in the background."
                        }),
                    );
                    tauri::async_runtime::spawn_blocking(move || {
                        let result = timeline::record_snapshot_cancellable(
                            &directory,
                            &timeline_summary,
                            &directories,
                            &files,
                            retention_days,
                            &|| {
                                timeline_state
                                    .cancelled
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            },
                        );
                        match result {
                            Ok(_) => {
                                let _ = timeline_app.emit(
                                    "timeline-status",
                                    serde_json::json!({
                                        "status": "saved",
                                        "message": "Timeline history saved."
                                    }),
                                );
                            }
                            Err(error) => {
                                let _ = timeline_app.emit(
                                    "timeline-status",
                                    serde_json::json!({
                                        "status": "warning",
                                        "message": format!("Timeline snapshot was not saved: {error}")
                                    }),
                                );
                            }
                        }
                    });
                }
                None => {
                    let _ = app.emit(
                        "timeline-status",
                        serde_json::json!({
                            "status": "warning",
                            "message": "Timeline storage directory is unavailable."
                        }),
                    );
                }
            }
        }
        *scan_state
            .last_summary
            .lock()
            .map_err(|_| "Scan state unavailable")? = Some(summary.clone());
        Ok(summary)
    })
    .await
    .map_err(|error| format!("Scan worker stopped unexpectedly: {error}"))?
}

#[tauri::command]
async fn get_timeline(
    app: tauri::AppHandle,
    state: State<'_, ScanState>,
    range_days: u32,
) -> Result<models::TimelineResponse, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Timeline storage directory is unavailable: {error}"))?;
    let active_root = state
        .root
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clone();
    let guard = AtomicGuard::acquire(
        state.operation_running.clone(),
        "Another filesystem operation is already running.",
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        Ok(timeline::response(
            &directory,
            active_root.as_deref(),
            range_days,
        ))
    })
    .await
    .map_err(|error| format!("Timeline worker stopped unexpectedly: {error}"))?
}
#[tauri::command]
fn cancel_scan(state: State<ScanState>) {
    state
        .cancelled
        .store(true, std::sync::atomic::Ordering::Relaxed)
}
#[tauri::command]
fn get_files(
    state: State<ScanState>,
    minimum_bytes: Option<u64>,
) -> Result<Vec<models::FileEntry>, String> {
    if state
        .last_summary
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .as_ref()
        .is_some_and(|summary| summary.cancelled)
    {
        return Ok(vec![]);
    }
    let minimum = minimum_bytes.unwrap_or(100 * 1024 * 1024);
    Ok(state
        .files
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .iter()
        .filter(|file| file.size >= minimum)
        .cloned()
        .collect())
}
#[tauri::command]
fn find_inspector_file_matches(
    state: State<ScanState>,
    query: String,
) -> Result<Vec<models::FileInspectorMatch>, String> {
    let root = state
        .root
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clone();
    let files = state.files.lock().map_err(|_| "Scan state unavailable")?;
    Ok(file_inspector::find_matches(
        &files,
        root.as_deref(),
        &query,
    ))
}

#[tauri::command]
async fn inspect_file_contents(
    state: State<'_, ScanState>,
    path: String,
    allow_outside_scope: bool,
) -> Result<models::FileInspectionResult, String> {
    let scan_state = state.inner().clone();
    let root = scan_state
        .root
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        file_inspector::inspect(&path, root.as_deref(), allow_outside_scope)
    })
    .await
    .map_err(|error| format!("File Inspector worker stopped unexpectedly: {error}"))?
}
#[tauri::command]
async fn get_developer_storage(
    state: State<'_, ScanState>,
) -> Result<models::DeveloperStorageReport, String> {
    let scan_state = state.inner().clone();
    let guard = AtomicGuard::acquire(
        scan_state.operation_running.clone(),
        "Another filesystem operation is already running.",
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let files = scan_state
            .files
            .lock()
            .map_err(|_| "Scan state unavailable")?
            .clone();
        let directories = scan_state
            .directories
            .lock()
            .map_err(|_| "Scan state unavailable")?
            .clone();
        if scan_state
            .root
            .lock()
            .map_err(|_| "Scan state unavailable")?
            .is_none()
        {
            return Err("Run a scan first to analyze developer storage.".into());
        }
        Ok(developer_storage::analyze(&files, &directories))
    })
    .await
    .map_err(|e| format!("Developer Storage worker stopped unexpectedly: {e}"))?
}
#[tauri::command]
fn get_tree_children(
    state: State<ScanState>,
    path: String,
) -> Result<Vec<models::TreeNode>, String> {
    let root = state
        .root
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clone()
        .ok_or("Scan a location first.")?;
    let requested = Path::new(&path);
    if !requested.starts_with(Path::new(&root)) {
        return Err("Tree location is outside the selected scan scope.".into());
    }
    let directories = state
        .directories
        .lock()
        .map_err(|_| "Scan state unavailable")?;
    if !directories.contains_key(&path) {
        return Err("This directory is no longer part of the active scan.".into());
    }
    let files = state.files.lock().map_err(|_| "Scan state unavailable")?;
    let mut children: Vec<models::TreeNode> = directories
        .values()
        .filter(|directory| {
            Path::new(&directory.path).parent() == Some(requested)
                && Path::new(&directory.path) != requested
        })
        .map(|directory| models::TreeNode {
            path: directory.path.clone(),
            name: directory.name.clone(),
            size: directory.size,
            is_directory: true,
        })
        .chain(
            files
                .iter()
                .filter(|file| Path::new(&file.path).parent() == Some(requested))
                .map(|file| models::TreeNode {
                    path: file.path.clone(),
                    name: file.name.clone(),
                    size: file.size,
                    is_directory: false,
                }),
        )
        .collect();
    children.sort_by(|a, b| b.size.cmp(&a.size));
    children.truncate(250);
    Ok(children)
}
#[tauri::command]
async fn find_duplicates(
    app: tauri::AppHandle,
    state: State<'_, ScanState>,
) -> Result<Vec<models::DuplicateGroup>, String> {
    let guard = AtomicGuard::acquire(
        state.operation_running.clone(),
        "Another filesystem operation is already running.",
    )?;
    let files = state
        .files
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clone();
    state
        .duplicate_cancelled
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let cancellation = state.duplicate_cancelled.clone();
    let groups = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        duplicates::find_with_progress(
            &files,
            |progress| {
                let _ = app.emit("duplicate-progress", progress);
            },
            || cancellation.load(std::sync::atomic::Ordering::Relaxed),
        )
    })
    .await
    .map_err(|error| format!("Duplicate worker stopped unexpectedly: {error}"))?;
    *state
        .duplicate_groups
        .lock()
        .map_err(|_| "Scan state unavailable")? = groups.clone();
    state.duplicate_analysis_complete.store(
        !state
            .duplicate_cancelled
            .load(std::sync::atomic::Ordering::Acquire),
        std::sync::atomic::Ordering::Release,
    );
    Ok(groups)
}
#[tauri::command]
fn cancel_duplicates(state: State<ScanState>) {
    state
        .duplicate_cancelled
        .store(true, std::sync::atomic::Ordering::Relaxed);
}
#[tauri::command]
async fn ask_ai_copilot(
    app: tauri::AppHandle,
    state: State<'_, ScanState>,
    question: String,
    provider_id: String,
    model: Option<String>,
) -> Result<models::AiCopilotResponse, String> {
    let scan_state = state.inner().clone();
    let guard = AtomicGuard::acquire(
        scan_state.operation_running.clone(),
        "Another filesystem operation is already running.",
    )?;
    scan_state
        .ai_cancelled
        .store(false, std::sync::atomic::Ordering::Release);
    let app_data = app.path().app_data_dir().ok();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        ai::answer(
            &scan_state,
            app_data.as_deref(),
            &question,
            &provider_id,
            model.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("AI Copilot worker stopped unexpectedly: {e}"))?
}
#[tauri::command]
fn cancel_ai_copilot(state: State<ScanState>) {
    state
        .ai_cancelled
        .store(true, std::sync::atomic::Ordering::Release);
}
#[tauri::command]
fn ai_provider_status(
    app: tauri::AppHandle,
    provider_id: String,
    model: Option<String>,
) -> Result<models::AiProviderStatus, String> {
    let app_data = app.path().app_data_dir().ok();
    ai::provider_status_response(&provider_id, model, app_data.as_deref())
}

#[tauri::command]
fn save_ai_provider_key(provider_id: String, api_key: String) -> Result<(), String> {
    ai::save_provider_key(&provider_id, &api_key)
}

#[tauri::command]
fn delete_ai_provider_key(provider_id: String) -> Result<(), String> {
    ai::delete_provider_key(&provider_id)
}

#[tauri::command]
async fn test_ai_provider(provider_id: String, model: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        ai::test_provider_key(&provider_id, model.as_deref())
    })
    .await
    .map_err(|error| format!("AI provider test stopped unexpectedly: {error}"))?
}
#[tauri::command]
fn get_recommendations(state: State<ScanState>) -> Result<Vec<models::Recommendation>, String> {
    let root = state
        .root
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clone()
        .ok_or("Scan a location first.")?;
    Ok(cleanup::recommendations(
        &state.files.lock().map_err(|_| "Scan state unavailable")?,
        &root,
    ))
}

fn rescue_path_is_current(root: &Path, file: &models::FileEntry) -> bool {
    let path = Path::new(&file.path);
    if categories::protected(path) {
        return false;
    }
    let Ok(real) = path.canonicalize() else {
        return false;
    };
    if real == root || !real.starts_with(root) || categories::protected(&real) {
        return false;
    }
    path.symlink_metadata().is_ok_and(|metadata| {
        !metadata.file_type().is_symlink()
            && !is_reparse_point(&metadata)
            && metadata.is_file()
            && metadata.len() == file.size
            && modified_string(&metadata) == file.modified
    })
}

#[tauri::command]
async fn build_rescue_plan(
    state: State<'_, ScanState>,
    target_bytes: u64,
) -> Result<models::RescuePlan, String> {
    if target_bytes == 0 {
        return Err("Choose a recovery target greater than zero.".into());
    }
    let scan_state = state.inner().clone();
    let guard = AtomicGuard::acquire(
        scan_state.operation_running.clone(),
        "Another filesystem operation is already running.",
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let root = scan_state
            .root
            .lock()
            .map_err(|_| "Scan state unavailable")?
            .clone()
            .ok_or("Run a scan first to build a Rescue Plan.")?;
        let root_path = Path::new(&root)
            .canonicalize()
            .map_err(|_| "The active scan location is no longer available. Run a new scan.")?;
        let files = scan_state
            .files
            .lock()
            .map_err(|_| "Scan state unavailable")?;
        let duplicate_groups = scan_state
            .duplicate_groups
            .lock()
            .map_err(|_| "Scan state unavailable")?;
        let cleanup = cleanup::recommendations(&files, &root);
        let relevant_paths: HashSet<&str> = duplicate_groups
            .iter()
            .flat_map(|group| group.files.iter().map(|file| file.path.as_str()))
            .chain(cleanup.iter().map(|item| item.file.path.as_str()))
            .collect();
        let relevant_file_count = files
            .iter()
            .filter(|file| {
                file.size >= 100 * 1024 * 1024
                    || relevant_paths.contains(file.path.as_str())
            })
            .count();
        let valid_paths: HashSet<String> = files
            .iter()
            .filter(|file| file.size >= 100 * 1024 * 1024 || relevant_paths.contains(file.path.as_str()))
            .filter(|file| rescue_path_is_current(&root_path, file))
            .map(|file| file.path.clone())
            .collect();
        let stale_items = relevant_file_count.saturating_sub(valid_paths.len()) as u64;
        let free = fs2::available_space(&root_path)
            .map_err(|error| format!("Could not read current free disk space: {error}"))?;
        let duplicate_analysis_complete = scan_state
            .duplicate_analysis_complete
            .load(std::sync::atomic::Ordering::Acquire);
        let mut plan = rescue::build(
            &files,
            &duplicate_groups,
            &cleanup,
            target_bytes,
            free,
            chrono::Utc::now(),
            &valid_paths,
            stale_items,
        );
        let directories = scan_state.directories.lock().map_err(|_| "Scan state unavailable")?;
        let developer_report = developer_storage::analyze(&files, &directories);
        if developer_report.potential_review_bytes > 0 {
            plan.warnings.push(format!(
                "Developer Storage identified {} bytes worth reviewing separately. These bytes are not added to this Rescue Plan because developer-generated storage is not automatically recoverable.",
                developer_report.potential_review_bytes
            ));
        }
        if !duplicate_analysis_complete {
            plan.warnings.push(
                "Exact duplicates are not included yet. Run Duplicate analysis to verify them with SHA-256, then rebuild this plan."
                    .into(),
            );
        }
        Ok(plan)
    })
    .await
    .map_err(|error| format!("Rescue Plan worker stopped unexpectedly: {error}"))?
}
fn move_to_trash_inner(
    state: &ScanState,
    paths: Vec<String>,
    timeline_directory: Option<PathBuf>,
) -> Result<serde_json::Value, String> {
    let root = state
        .root
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .clone()
        .ok_or("Scan a location first.")?;
    let root_path = Path::new(&root)
        .canonicalize()
        .map_err(|_| "Scan location is no longer available.")?;
    let scanned_files: HashMap<String, models::FileEntry> = state
        .files
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .iter()
        .cloned()
        .map(|file| (file.path.clone(), file))
        .collect();
    let application_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let mut deleted = vec![];
    let mut errors = vec![];
    let mut warnings = vec![];
    let requested_paths: HashSet<String> = paths.iter().cloned().collect();
    let mut duplicate_keep_required: HashSet<String> = HashSet::new();
    for group in state
        .duplicate_groups
        .lock()
        .map_err(|_| "Scan state unavailable")?
        .iter()
    {
        if group.files.len() > 1
            && group
                .files
                .iter()
                .all(|file| requested_paths.contains(&file.path))
        {
            errors.push(format!(
                "Duplicate group {}: keep at least one verified copy",
                group.hash
            ));
            duplicate_keep_required.extend(group.files.iter().map(|file| file.path.clone()));
        }
    }
    let mut seen = HashSet::new();
    for raw in paths {
        if !seen.insert(raw.clone()) {
            errors.push(format!("{}: duplicate selection ignored", raw));
            continue;
        }
        if duplicate_keep_required.contains(&raw) {
            continue;
        }
        let p = Path::new(&raw);
        let Some(scanned) = scanned_files.get(&raw) else {
            errors.push(format!("{}: not present in the active scan", raw));
            continue;
        };
        match validate_delete_candidate(&root_path, p, scanned, application_dir.as_deref()) {
            Ok(real) => match trash::delete(&real) {
                Ok(_) => deleted.push(raw),
                Err(e) => errors.push(format!("{}: {}", raw, e)),
            },
            Err(error) => errors.push(error),
        }
    }
    if !deleted.is_empty() {
        let deleted_paths: HashSet<&str> = deleted.iter().map(String::as_str).collect();
        let mut files = state.files.lock().map_err(|_| "Scan state unavailable")?;
        let removed: Vec<_> = files
            .iter()
            .filter(|file| deleted_paths.contains(file.path.as_str()))
            .cloned()
            .collect();
        let removed_bytes = removed
            .iter()
            .fold(0u64, |total, file| total.saturating_add(file.size));
        let removed_count = removed.len() as u64;
        files.retain(|file| !deleted_paths.contains(file.path.as_str()));
        drop(files);
        let mut directories = state
            .directories
            .lock()
            .map_err(|_| "Scan state unavailable")?;
        for file in &removed {
            let mut ancestor = Path::new(&file.path).parent();
            while let Some(path) = ancestor {
                if !path.starts_with(&root_path) {
                    break;
                }
                if let Some(directory) = directories.get_mut(&path.to_string_lossy().to_string()) {
                    directory.size = directory.size.saturating_sub(file.size);
                }
                if path == root_path {
                    break;
                }
                ancestor = path.parent();
            }
        }
        drop(directories);
        let mut groups = state
            .duplicate_groups
            .lock()
            .map_err(|_| "Scan state unavailable")?;
        for group in groups.iter_mut() {
            group
                .files
                .retain(|file| !deleted_paths.contains(file.path.as_str()));
            group.recoverable_size = group
                .size
                .saturating_mul((group.files.len() as u64).saturating_sub(1));
        }
        groups.retain(|group| group.files.len() > 1);
        drop(groups);
        if let Some(directory) = timeline_directory {
            let volume_root = root_path
                .ancestors()
                .last()
                .unwrap_or(root_path.as_path())
                .to_string_lossy()
                .into_owned();
            if let Err(error) = timeline::record_deletion(
                &directory,
                &volume_root,
                &root,
                removed_bytes,
                removed_count,
            ) {
                warnings.push(format!(
                    "Cleanup succeeded, but Timeline attribution was not saved: {error}"
                ));
            }
        }
    }
    Ok(serde_json::json!({"deleted":deleted,"errors":errors,"warnings":warnings}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spacepilot-{name}-{nonce}"))
    }

    fn scanned(path: &Path) -> models::FileEntry {
        let metadata = path.metadata().unwrap();
        models::FileEntry {
            path: path.to_string_lossy().into_owned(),
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            extension: "bin".into(),
            size: metadata.len(),
            modified: modified_string(&metadata),
            category: "Other".into(),
            is_directory: false,
        }
    }

    #[test]
    fn deletion_validation_enforces_scan_scope() {
        let base = fixture("scope");
        let root = base.join("scan");
        let outside = base.join("outside.bin");
        let inside = root.join("inside.bin");
        fs::create_dir_all(&root).unwrap();
        fs::write(&inside, b"safe fixture").unwrap();
        fs::write(&outside, b"outside fixture").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        assert!(
            validate_delete_candidate(&canonical_root, &inside, &scanned(&inside), None).is_ok()
        );
        assert!(
            validate_delete_candidate(&canonical_root, &outside, &scanned(&outside), None).is_err()
        );
        assert!(inside.exists());
        assert!(outside.exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn deletion_validation_rejects_changed_missing_and_directory_entries() {
        let base = fixture("changed");
        let root = base.join("scan");
        let file = root.join("file.bin");
        fs::create_dir_all(&root).unwrap();
        fs::write(&file, b"before").unwrap();
        let snapshot = scanned(&file);
        fs::write(&file, b"changed content").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        assert!(validate_delete_candidate(&canonical_root, &file, &snapshot, None).is_err());
        let mut directory = scanned(&file);
        directory.is_directory = true;
        assert!(validate_delete_candidate(&canonical_root, &file, &directory, None).is_err());
        fs::remove_file(&file).unwrap();
        assert!(validate_delete_candidate(&canonical_root, &file, &snapshot, None).is_err());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn deletion_validation_rejects_unscanned_and_application_files() {
        let base = fixture("identity");
        let root = base.join("scan");
        let file = root.join("file.bin");
        fs::create_dir_all(&root).unwrap();
        fs::write(&file, b"fixture").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let mut wrong = scanned(&file);
        wrong.path = root.join("different.bin").to_string_lossy().into_owned();
        assert!(validate_delete_candidate(&canonical_root, &file, &wrong, None).is_err());
        assert!(validate_delete_candidate(
            &canonical_root,
            &file,
            &scanned(&file),
            Some(&canonical_root),
        )
        .is_err());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn operation_guard_rejects_parallel_actions_and_releases_on_drop() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let guard = AtomicGuard::acquire(flag.clone(), "busy").unwrap();
        assert!(AtomicGuard::acquire(flag.clone(), "busy").is_err());
        drop(guard);
        assert!(AtomicGuard::acquire(flag, "busy").is_ok());
    }

    #[test]
    fn deletion_rejects_removing_every_file_in_duplicate_group() {
        let base = fixture("duplicate-keep-one");
        let root = base.join("scan");
        let one = root.join("one.bin");
        let two = root.join("two.bin");
        fs::create_dir_all(&root).unwrap();
        fs::write(&one, b"same").unwrap();
        fs::write(&two, b"same").unwrap();
        let state = ScanState::default();
        *state.root.lock().unwrap() = Some(root.canonicalize().unwrap().to_string_lossy().into());
        let one_entry = scanned(&one);
        let two_entry = scanned(&two);
        *state.files.lock().unwrap() = vec![one_entry.clone(), two_entry.clone()];
        *state.duplicate_groups.lock().unwrap() = vec![models::DuplicateGroup {
            hash: "verified-hash".into(),
            size: one_entry.size,
            files: vec![one_entry.clone(), two_entry.clone()],
            recoverable_size: one_entry.size,
        }];

        let result = move_to_trash_inner(
            &state,
            vec![one_entry.path.clone(), two_entry.path.clone()],
            None,
        )
        .unwrap();

        assert!(result["deleted"].as_array().unwrap().is_empty());
        assert!(result["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("keep at least one")));
        assert!(one.exists());
        assert!(two.exists());
        let _ = fs::remove_dir_all(base);
    }
}
#[tauri::command]
async fn move_to_trash(
    app: tauri::AppHandle,
    state: State<'_, ScanState>,
    paths: Vec<String>,
) -> Result<serde_json::Value, String> {
    let scan_state = state.inner().clone();
    let timeline_directory = app.path().app_data_dir().ok();
    let guard = AtomicGuard::acquire(
        scan_state.operation_running.clone(),
        "Another filesystem operation is already running.",
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        move_to_trash_inner(&scan_state, paths, timeline_directory)
    })
    .await
    .map_err(|error| format!("Cleanup worker stopped unexpectedly: {error}"))?
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(ScanState::default())
        .invoke_handler(tauri::generate_handler![
            select_folder,
            select_inspector_file,
            scan_folder,
            cancel_scan,
            get_files,
            find_inspector_file_matches,
            inspect_file_contents,
            get_developer_storage,
            get_tree_children,
            find_duplicates,
            cancel_duplicates,
            get_recommendations,
            build_rescue_plan,
            get_timeline,
            move_to_trash,
            ask_ai_copilot,
            cancel_ai_copilot,
            ai_provider_status,
            save_ai_provider_key,
            delete_ai_provider_key,
            test_ai_provider,
            commercial::secure_session_status,
            commercial::secure_logout_local,
            commercial::stable_device_id,
            commercial::account_auth,
            commercial::backend_request,
            commercial::refresh_session,
            commercial::account_logout,
            commercial::verify_cached_entitlement,
            commercial::cache_entitlement
        ])
        .run(tauri::generate_context!())
        .expect("SpacePilot failed to start")
}
