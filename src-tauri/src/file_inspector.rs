use crate::models::{FileEntry, FileInspectionResult, FileInspectorMatch};
use chrono::{DateTime, Utc};
use std::{
    collections::HashSet,
    fs::{self, Metadata},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_EXTRACTED_CHARS: usize = 60_000;

const TEXT_EXTENSIONS: &[&str] = &["txt", "md", "csv", "json", "log"];
const UNSUPPORTED_DOCUMENT_EXTENSIONS: &[&str] = &["pdf", "docx"];
const SENSITIVE_NAMES: &[&str] = &[
    ".env",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "credentials",
    "credentials.json",
    "known_hosts",
];

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}

fn modified_string(metadata: &Metadata) -> Option<String> {
    metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Utc>::from(time).to_rfc3339())
}

fn friendly_label(path: &str, root: Option<&str>) -> String {
    let path_value = Path::new(path);
    if let Some(root_value) = root {
        if let Ok(relative) = path_value.strip_prefix(root_value) {
            return relative
                .to_string_lossy()
                .trim_start_matches('\\')
                .to_string();
        }
    }
    name(path_value)
}

fn is_sensitive_path(path: &Path) -> bool {
    let file_name = name(path).to_ascii_lowercase();
    if SENSITIVE_NAMES.iter().any(|value| *value == file_name) {
        return true;
    }
    let lowered = path.to_string_lossy().to_ascii_lowercase();
    lowered.contains("\\.ssh\\")
        || lowered.contains("\\appdata\\roaming\\gcloud\\")
        || lowered.contains("\\appdata\\roaming\\aws\\")
        || lowered.contains("\\appdata\\roaming\\docker\\")
}

fn contains_secret_pattern(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let markers = [
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "aws_secret_access_key",
        "private_key",
        "client_secret",
        "api_key",
        "access_token",
        "refresh_token",
        "password=",
        "secret=",
        "sk_live_",
        "sk_test_",
    ];
    markers.iter().any(|marker| lowered.contains(marker))
}

fn category_for_extension(extension: &str) -> String {
    crate::categories::category(extension)
}

pub fn find_matches(
    files: &[FileEntry],
    root: Option<&str>,
    query: &str,
) -> Vec<FileInspectorMatch> {
    let wanted = query.trim().to_ascii_lowercase();
    if wanted.is_empty() {
        return vec![];
    }
    let mut seen = HashSet::new();
    let mut matches: Vec<_> = files
        .iter()
        .filter(|file| !file.is_directory && file.name.to_ascii_lowercase() == wanted)
        .filter(|file| seen.insert(file.path.clone()))
        .map(|file| FileInspectorMatch {
            path: file.path.clone(),
            name: file.name.clone(),
            extension: file.extension.clone(),
            size: file.size,
            modified: file.modified.clone(),
            category: file.category.clone(),
            label: friendly_label(&file.path, root),
        })
        .collect();
    matches.sort_by(|a, b| a.label.cmp(&b.label));
    matches.truncate(20);
    matches
}

fn validate_path(root: Option<&str>, requested: &Path) -> Result<PathBuf, String> {
    if is_sensitive_path(requested) {
        return Err(
            "This file appears to contain credentials or secrets and cannot be inspected.".into(),
        );
    }
    let canonical = requested
        .canonicalize()
        .map_err(|_| "The selected file is no longer available.".to_string())?;
    if let Some(root_value) = root {
        let root_path = Path::new(root_value)
            .canonicalize()
            .map_err(|_| "The active scan location is no longer available.".to_string())?;
        if !canonical.starts_with(root_path) {
            return Err("Choose a file inside the active scan scope, or select it explicitly with the file picker.".into());
        }
    }
    let metadata = canonical
        .symlink_metadata()
        .map_err(|_| "The selected file cannot be read.".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(
            "Symbolic links and reparse points are not eligible for file inspection.".into(),
        );
    }
    Ok(canonical)
}

pub fn inspect(
    path: &str,
    root: Option<&str>,
    allow_outside_scope: bool,
) -> Result<FileInspectionResult, String> {
    let requested = Path::new(path);
    let root_scope = if allow_outside_scope { None } else { root };
    let canonical = validate_path(root_scope, requested)?;
    let metadata = canonical
        .metadata()
        .map_err(|_| "The selected file cannot be read.".to_string())?;
    if !metadata.is_file() {
        return Err("Only regular files can be inspected.".into());
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "This file is too large for beta inspection. The current limit is {} MB.",
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }
    let extension = extension(&canonical);
    if UNSUPPORTED_DOCUMENT_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "SpacePilot cannot inspect .{} files yet. PDF/DOCX support needs a safe parser before beta release.",
            extension
        ));
    }
    if !TEXT_EXTENSIONS.contains(&extension.as_str()) {
        return Err("SpacePilot cannot inspect this file type yet.".into());
    }
    let mut file = fs::File::open(&canonical)
        .map_err(|_| "The selected file could not be opened.".to_string())?;
    let mut buffer = Vec::new();
    file.by_ref()
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| "The selected file could not be read.".to_string())?;
    let text = String::from_utf8(buffer).map_err(|_| {
        "This file is not valid UTF-8 text and cannot be safely inspected yet.".to_string()
    })?;
    let mut truncated = false;
    let mut extracted: String = text.chars().take(MAX_EXTRACTED_CHARS).collect();
    if text.chars().count() > MAX_EXTRACTED_CHARS {
        truncated = true;
        extracted.push_str("\n\n[SpacePilot truncated this file for safety.]");
    }
    let secret_detected = contains_secret_pattern(&extracted);
    let lines = extracted
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let words = extracted.split_whitespace().count();
    let preview = extracted
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("No readable text was found.")
        .trim();
    let mut warnings = vec![];
    if truncated {
        warnings.push("Only the first part of this file was inspected.".into());
    }
    if secret_detected {
        warnings.push(
            "Potential secret material was detected. Cloud AI is blocked for this file.".into(),
        );
    }
    Ok(FileInspectionResult {
        path: canonical.to_string_lossy().to_string(),
        name: name(&canonical),
        extension: extension.clone(),
        size: metadata.len(),
        modified: modified_string(&metadata),
        category: category_for_extension(&extension),
        extracted_characters: extracted.chars().count(),
        truncated,
        local_summary: format!(
            "Local inspection found {lines} non-empty line(s) and about {words} word(s). First readable line: \"{preview}\""
        ),
        warnings,
        cloud_allowed: !secret_detected,
        cloud_block_reason: if secret_detected {
            Some("This file appears to contain sensitive credentials and cannot be sent to cloud AI.".into())
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FileEntry;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spacepilot-inspector-{name}-{nonce}"))
    }

    #[test]
    fn finds_exact_filename_matches() {
        let files = vec![
            FileEntry {
                name: "budget.pdf".into(),
                path: "C:\\scan\\budget.pdf".into(),
                extension: "pdf".into(),
                size: 1,
                modified: None,
                category: "Documents".into(),
                is_directory: false,
            },
            FileEntry {
                name: "other.pdf".into(),
                path: "C:\\scan\\other.pdf".into(),
                extension: "pdf".into(),
                size: 1,
                modified: None,
                category: "Documents".into(),
                is_directory: false,
            },
        ];
        assert_eq!(
            find_matches(&files, Some("C:\\scan"), "budget.pdf").len(),
            1
        );
        assert!(find_matches(&files, Some("C:\\scan"), "missing.pdf").is_empty());
    }

    #[test]
    fn extracts_bounded_text_and_blocks_secrets() {
        let root = temp("text");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("note.txt");
        std::fs::write(&file, "hello world\napi_key=secret").unwrap();
        let result = inspect(
            &file.to_string_lossy(),
            Some(&root.to_string_lossy()),
            false,
        )
        .unwrap();
        assert!(!result.cloud_allowed);
        assert!(result.local_summary.contains("hello world"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_outside_scope_and_unsupported_types() {
        let root = temp("scope");
        let outside = temp("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let file = outside.join("report.exe");
        std::fs::write(&file, "nope").unwrap();
        assert!(inspect(
            &file.to_string_lossy(),
            Some(&root.to_string_lossy()),
            false
        )
        .is_err());
        assert!(inspect(&file.to_string_lossy(), None, true).is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn explicit_picker_can_inspect_outside_scan_scope() {
        let root = temp("picker");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("picked.md");
        std::fs::write(&file, "# Title\nBody").unwrap();
        let result = inspect(&file.to_string_lossy(), None, true).unwrap();
        assert_eq!(result.name, "picked.md");
        let _ = std::fs::remove_dir_all(root);
    }
}
