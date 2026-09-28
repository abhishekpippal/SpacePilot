use crate::{categories, models::*};
use std::path::Path;

fn normalized_components(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter(|component| !component.is_empty())
        .map(|component| component.trim_end_matches(':').to_ascii_lowercase())
        .collect()
}

fn path_is_within(root: &str, candidate: &str) -> bool {
    let root = normalized_components(root);
    let candidate = normalized_components(candidate);
    !root.is_empty()
        && candidate.len() >= root.len()
        && candidate
            .iter()
            .zip(root.iter())
            .all(|(left, right)| left == right)
}

pub fn recommendations(files: &[FileEntry], root: &str) -> Vec<Recommendation> {
    files
        .iter()
        .filter(|f| {
            let path = Path::new(&f.path);
            !f.is_directory && path_is_within(root, &f.path) && !categories::protected(path)
        })
        .filter_map(|f| {
            if f.category == "Temporary" {
                Some(Recommendation {
                    file: f.clone(),
                    reason: "Temporary or cache file within the selected location.".into(),
                    risk: "Safe".into(),
                })
            } else if f.size >= 1_000_000_000
                && matches!(f.category.as_str(), "Archives" | "Videos" | "Other")
            {
                Some(Recommendation {
                    file: f.clone(),
                    reason: "Large non-system file; review before removing.".into(),
                    risk: "Review".into(),
                })
            } else {
                None
            }
        })
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(path: &str, category: &str, size: u64) -> FileEntry {
        FileEntry {
            path: path.into(),
            name: "candidate".into(),
            extension: "tmp".into(),
            size,
            modified: None,
            category: category.into(),
            is_directory: false,
        }
    }
    #[test]
    fn recommends_temporary_files_inside_scope() {
        let result = recommendations(&[file("C:\\scan\\file.tmp", "Temporary", 10)], "C:\\scan");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].risk, "Safe");
    }
    #[test]
    fn rejects_out_of_scope_and_protected_files() {
        let files = [
            file("D:\\other\\file.tmp", "Temporary", 10),
            file("C:\\scan\\Windows\\file.tmp", "Temporary", 10),
        ];
        assert!(recommendations(&files, "C:\\scan").is_empty());
    }

    #[test]
    fn component_scope_check_rejects_similar_prefixes() {
        let files = [file("C:\\scanner\\file.tmp", "Temporary", 10)];
        assert!(recommendations(&files, "C:\\scan").is_empty());
    }

    #[test]
    fn does_not_recommend_directory_records() {
        let mut directory = file("C:\\scan\\cache.tmp", "Temporary", 10);
        directory.is_directory = true;
        assert!(recommendations(&[directory], "C:\\scan").is_empty());
    }
}
