use crate::models::*;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
};

fn matches_scan_snapshot(file: &FileEntry, metadata: &std::fs::Metadata) -> bool {
    let modified = metadata
        .modified()
        .ok()
        .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339());
    metadata.is_file() && metadata.len() == file.size && modified == file.modified
}

fn hash<F, C>(file: &FileEntry, mut on_bytes: F, is_cancelled: &C) -> Result<String, String>
where
    F: FnMut(u64),
    C: Fn() -> bool,
{
    let link_metadata = std::fs::symlink_metadata(&file.path).map_err(|e| e.to_string())?;
    if link_metadata.file_type().is_symlink() || !matches_scan_snapshot(file, &link_metadata) {
        return Err("file changed after scanning".into());
    }
    let f = File::open(&file.path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        if is_cancelled() {
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        on_bytes(n as u64);
    }
    let digest = format!("{:x}", h.finalize());
    let final_metadata = std::fs::metadata(&file.path).map_err(|e| e.to_string())?;
    if !matches_scan_snapshot(file, &final_metadata) {
        return Err("file changed while hashing".into());
    }
    Ok(digest)
}
pub fn find_with_progress<F, C>(
    files: &[FileEntry],
    mut on_progress: F,
    is_cancelled: C,
) -> Vec<DuplicateGroup>
where
    F: FnMut(DuplicateProgress),
    C: Fn() -> bool,
{
    let mut sizes: HashMap<u64, Vec<FileEntry>> = HashMap::new();
    for f in files {
        if f.size > 0 {
            sizes.entry(f.size).or_default().push(f.clone())
        }
    }
    let candidates: Vec<(u64, Vec<FileEntry>)> = sizes
        .into_iter()
        .filter(|(_, group)| group.len() > 1)
        .collect();
    let total_files = candidates.iter().map(|(_, group)| group.len() as u64).sum();
    let total_bytes = candidates
        .iter()
        .map(|(size, group)| size.saturating_mul(group.len() as u64))
        .sum();
    let mut processed_files = 0;
    let mut bytes_hashed = 0;
    on_progress(DuplicateProgress {
        phase: "Hashing candidates".into(),
        processed_files,
        total_files,
        bytes_hashed,
        total_bytes,
        current_path: String::new(),
    });
    let mut hashes: HashMap<(u64, String), Vec<FileEntry>> = HashMap::new();
    for (size, group) in candidates {
        for f in group {
            if is_cancelled() {
                return build_groups(hashes);
            }
            let current_path = f.path.clone();
            let mut bytes_since_update = 0;
            let result = hash(
                &f,
                |read| {
                    bytes_hashed += read;
                    bytes_since_update += read;
                    if bytes_since_update >= 8 * 1024 * 1024 {
                        bytes_since_update = 0;
                        on_progress(DuplicateProgress {
                            phase: "Hashing candidates".into(),
                            processed_files,
                            total_files,
                            bytes_hashed,
                            total_bytes,
                            current_path: current_path.clone(),
                        });
                    }
                },
                &is_cancelled,
            );
            if is_cancelled() {
                return build_groups(hashes);
            }
            processed_files += 1;
            on_progress(DuplicateProgress {
                phase: "Hashing candidates".into(),
                processed_files,
                total_files,
                bytes_hashed,
                total_bytes,
                current_path: current_path.clone(),
            });
            if let Ok(h) = result {
                hashes.entry((size, h)).or_default().push(f)
            }
        }
    }
    build_groups(hashes)
}
fn build_groups(hashes: HashMap<(u64, String), Vec<FileEntry>>) -> Vec<DuplicateGroup> {
    hashes
        .into_iter()
        .filter_map(|((size, hash), files)| {
            let files: Vec<_> = files
                .into_iter()
                .filter(|file| {
                    std::fs::metadata(&file.path)
                        .is_ok_and(|metadata| matches_scan_snapshot(file, &metadata))
                })
                .collect();
            if files.len() > 1 {
                Some(DuplicateGroup {
                    hash,
                    size,
                    recoverable_size: size * (files.len() as u64 - 1),
                    files,
                })
            } else {
                None
            }
        })
        .collect()
}
#[cfg(test)]
pub fn find(files: &[FileEntry]) -> Vec<DuplicateGroup> {
    find_with_progress(files, |_| {}, || false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn fixture(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spacepilot-duplicate-{name}-{nonce}"))
    }

    fn item(path: PathBuf) -> FileEntry {
        let metadata = path.metadata().unwrap();
        FileEntry {
            path: path.to_string_lossy().into(),
            name: "x".into(),
            extension: "bin".into(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()),
            category: "Other".into(),
            is_directory: false,
        }
    }
    #[test]
    fn confirms_duplicate_content_with_sha256() {
        let base = fixture("content");
        fs::create_dir_all(&base).unwrap();
        let a = base.join("a.bin");
        let b = base.join("b.bin");
        fs::write(&a, b"abc").unwrap();
        fs::write(&b, b"abc").unwrap();
        let found = find(&[item(a), item(b)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].recoverable_size, 3);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn same_size_different_content_is_not_a_duplicate() {
        let base = fixture("different");
        fs::create_dir_all(&base).unwrap();
        let a = base.join("a.bin");
        let b = base.join("b.bin");
        fs::write(&a, b"abc").unwrap();
        fs::write(&b, b"xyz").unwrap();
        assert!(find(&[item(a), item(b)]).is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn changed_and_missing_candidates_are_skipped() {
        let base = fixture("changed");
        fs::create_dir_all(&base).unwrap();
        let a = base.join("a.bin");
        let b = base.join("b.bin");
        fs::write(&a, b"abc").unwrap();
        fs::write(&b, b"abc").unwrap();
        let a_snapshot = item(a.clone());
        let b_snapshot = item(b.clone());
        fs::write(&a, b"longer").unwrap();
        fs::remove_file(&b).unwrap();
        assert!(find(&[a_snapshot, b_snapshot]).is_empty());
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn cancellation_stops_before_hashing() {
        let base = fixture("cancel-before");
        fs::create_dir_all(&base).unwrap();
        let a = base.join("a.bin");
        let b = base.join("b.bin");
        fs::write(&a, b"abc").unwrap();
        fs::write(&b, b"abc").unwrap();
        let found = find_with_progress(&[item(a), item(b)], |_| {}, || true);
        assert!(found.is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn cancellation_interrupts_streaming_hash() {
        let base = fixture("cancel-during");
        fs::create_dir_all(&base).unwrap();
        let a = base.join("a.bin");
        let b = base.join("b.bin");
        let content = vec![7u8; 1024 * 1024];
        fs::write(&a, &content).unwrap();
        fs::write(&b, &content).unwrap();
        let checks = AtomicUsize::new(0);
        let found = find_with_progress(
            &[item(a), item(b)],
            |_| {},
            || checks.fetch_add(1, Ordering::Relaxed) >= 4,
        );
        assert!(found.is_empty());
        assert!(checks.load(Ordering::Relaxed) >= 5);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn hashing_missing_file_returns_an_error_without_panicking() {
        let path = fixture("missing").join("missing.bin");
        let fake = FileEntry {
            path: path.to_string_lossy().into_owned(),
            name: "missing.bin".into(),
            extension: "bin".into(),
            size: 1,
            modified: None,
            category: "Other".into(),
            is_directory: false,
        };
        assert!(hash(&fake, |_| {}, &|| false).is_err());
        assert!(!Path::new(&fake.path).exists());
    }
}
