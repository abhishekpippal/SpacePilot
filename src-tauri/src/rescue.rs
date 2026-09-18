use crate::{categories, models::*};
use chrono::{DateTime, Duration, Utc};
use std::{collections::HashSet, path::Path};

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

#[derive(Clone)]
struct Candidate {
    id: &'static str,
    category: &'static str,
    bytes: u64,
    risk: RescueRisk,
    reason: &'static str,
    action: &'static str,
    item_count: u64,
    item_paths: Vec<String>,
    accounted_paths: Vec<String>,
}

fn candidate(
    id: &'static str,
    category: &'static str,
    file: &FileEntry,
    reason: &'static str,
    action: &'static str,
) -> Candidate {
    Candidate {
        id,
        category,
        bytes: file.size,
        risk: RescueRisk::ReviewRequired,
        reason,
        action,
        item_count: 1,
        item_paths: vec![file.path.clone()],
        accounted_paths: vec![file.path.clone()],
    }
}

fn is_old_large(file: &FileEntry, now: DateTime<Utc>) -> bool {
    let Some(modified) = file
        .modified
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
    else {
        return false;
    };
    let age = now.signed_duration_since(modified.with_timezone(&Utc));
    (file.size >= GB && age >= Duration::days(180))
        || (file.size >= 500 * MB && age >= Duration::days(365))
}

fn is_archive(file: &FileEntry) -> bool {
    file.size >= 500 * MB
        && matches!(
            file.extension.to_ascii_lowercase().as_str(),
            "zip" | "rar" | "7z" | "tar" | "gz" | "iso"
        )
}

fn is_installer(file: &FileEntry) -> bool {
    if file.size < 100 * MB {
        return false;
    }
    let extension = file.extension.to_ascii_lowercase();
    let name = file.name.to_ascii_lowercase();
    let in_downloads = Path::new(&file.path).components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("downloads")
    });
    in_downloads
        && matches!(extension.as_str(), "msi" | "msp" | "exe")
        && (extension != "exe"
            || name.contains("setup")
            || name.contains("install")
            || name.contains("update"))
}

fn push_unique(bucket: &mut Vec<Candidate>, claimed: &mut HashSet<String>, item: Candidate) {
    if item
        .accounted_paths
        .iter()
        .all(|path| !claimed.contains(path))
    {
        claimed.extend(item.accounted_paths.iter().cloned());
        bucket.push(item);
    }
}

fn append_opportunity(opportunities: &mut Vec<RescueOpportunity>, candidate: Candidate) {
    if let Some(existing) = opportunities
        .iter_mut()
        .find(|opportunity| opportunity.id == candidate.id)
    {
        existing.estimated_bytes = existing.estimated_bytes.saturating_add(candidate.bytes);
        existing.item_count = existing.item_count.saturating_add(candidate.item_count);
        existing.item_paths.extend(candidate.item_paths);
    } else {
        opportunities.push(RescueOpportunity {
            id: candidate.id.into(),
            category: candidate.category.into(),
            estimated_bytes: candidate.bytes,
            risk: candidate.risk,
            reason: candidate.reason.into(),
            item_count: candidate.item_count,
            action_type: candidate.action.into(),
            item_paths: candidate.item_paths,
        });
    }
}

pub fn build(
    files: &[FileEntry],
    duplicate_groups: &[DuplicateGroup],
    cleanup: &[Recommendation],
    target_bytes: u64,
    current_free_bytes: u64,
    now: DateTime<Utc>,
    valid_paths: &HashSet<String>,
    stale_items: u64,
) -> RescuePlan {
    let eligible: Vec<&FileEntry> = files
        .iter()
        .filter(|file| {
            !file.is_directory
                && valid_paths.contains(&file.path)
                && !categories::protected(Path::new(&file.path))
        })
        .collect();
    let mut claimed = HashSet::new();
    let mut buckets: Vec<Vec<Candidate>> = vec![vec![], vec![], vec![], vec![], vec![], vec![]];

    for group in duplicate_groups {
        let mut members: Vec<&FileEntry> = group
            .files
            .iter()
            .filter(|file| {
                valid_paths.contains(&file.path)
                    && file.size == group.size
                    && !categories::protected(Path::new(&file.path))
            })
            .collect();
        members.sort_by(|left, right| left.path.cmp(&right.path));
        if members.len() < 2 {
            continue;
        }
        // Reserve every member from later opportunity categories. The byte estimate
        // still excludes one copy, but no later category can recommend that retained
        // copy and accidentally make the combined plan imply deleting the whole group.
        let accounted_paths: Vec<String> = members.iter().map(|file| file.path.clone()).collect();
        let bytes = group
            .size
            .saturating_mul((members.len() as u64).saturating_sub(1));
        push_unique(
            &mut buckets[0],
            &mut claimed,
            Candidate {
                id: "exact_duplicates",
                category: "Exact duplicates",
                bytes,
                risk: RescueRisk::Low,
                reason: "These files have identical content verified by SHA-256. At least one copy in every group is excluded from the recovery estimate.",
                action: "review_duplicates",
                item_count: 1,
                item_paths: members.iter().map(|file| file.path.clone()).collect(),
                accounted_paths,
            },
        );
    }

    for recommendation in cleanup {
        let file = &recommendation.file;
        if valid_paths.contains(&file.path) && !categories::protected(Path::new(&file.path)) {
            push_unique(
                &mut buckets[1],
                &mut claimed,
                candidate(
                    "cleanup_recommendations",
                    "Cleanup recommendations",
                    file,
                    "SpacePilot's existing deterministic cleanup rules surfaced these files. Review every item before removing it.",
                    "review_cleanup",
                ),
            );
        }
    }

    for file in &eligible {
        if is_old_large(file, now) {
            push_unique(
                &mut buckets[2],
                &mut claimed,
                candidate(
                    "old_large_files",
                    "Old large files",
                    file,
                    "These files are large and have not been modified for a long period. Age does not mean they are unnecessary, so review them first.",
                    "review_large_files",
                ),
            );
        }
    }
    for file in &eligible {
        if is_archive(file) {
            push_unique(
                &mut buckets[3],
                &mut claimed,
                candidate(
                    "large_archives",
                    "Large archives",
                    file,
                    "Large archives can remain after extraction. SpacePilot cannot determine whether you still need them, so review them first.",
                    "review_large_files",
                ),
            );
        }
    }
    for file in &eligible {
        if is_installer(file) {
            push_unique(
                &mut buckets[4],
                &mut claimed,
                candidate(
                    "installer_files",
                    "Installer files",
                    file,
                    "These installer-like files are in a Downloads folder. They may still be needed for reinstalling software, so review them first.",
                    "review_large_files",
                ),
            );
        }
    }
    for file in eligible {
        if file.size >= 500 * MB {
            push_unique(
                &mut buckets[5],
                &mut claimed,
                candidate(
                    "large_files",
                    "Other large files",
                    file,
                    "These files use substantial space. Size alone does not make a file unnecessary, so review it before removal.",
                    "review_large_files",
                ),
            );
        }
    }

    for bucket in &mut buckets {
        bucket.sort_by(|left, right| {
            right
                .bytes
                .cmp(&left.bytes)
                .then_with(|| left.accounted_paths.cmp(&right.accounted_paths))
        });
    }
    let total_reviewable_bytes = buckets
        .iter()
        .flatten()
        .fold(0u64, |total, item| total.saturating_add(item.bytes));
    let additional_bytes_needed = target_bytes.saturating_sub(current_free_bytes);
    let already_available = additional_bytes_needed == 0;
    let mut estimated_recoverable_bytes = 0u64;
    let mut opportunities = vec![];
    if !already_available {
        'priority: for bucket in buckets {
            for item in bucket {
                estimated_recoverable_bytes =
                    estimated_recoverable_bytes.saturating_add(item.bytes);
                append_opportunity(&mut opportunities, item);
                if estimated_recoverable_bytes >= additional_bytes_needed {
                    break 'priority;
                }
            }
        }
    }
    let target_achievable = already_available
        || current_free_bytes.saturating_add(total_reviewable_bytes) >= target_bytes;
    let confidence = if already_available
        || (target_achievable
            && opportunities
                .iter()
                .all(|opportunity| opportunity.risk == RescueRisk::Low))
    {
        "HIGH"
    } else if target_achievable {
        "MEDIUM"
    } else {
        "LOW"
    };
    let mut warnings = vec![];
    if stale_items > 0 {
        warnings.push(format!(
            "{stale_items} changed, moved, missing, or inaccessible item(s) were excluded."
        ));
    }
    if !target_achievable {
        warnings.push(format!(
            "SpacePilot could not identify enough responsible review opportunities for this target."
        ));
    }
    RescuePlan {
        target_bytes,
        current_free_bytes,
        additional_bytes_needed,
        estimated_recoverable_bytes,
        total_reviewable_bytes,
        target_achievable,
        already_available,
        opportunities,
        warnings,
        confidence: confidence.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64, extension: &str, days_old: i64) -> FileEntry {
        FileEntry {
            path: path.into(),
            name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            extension: extension.into(),
            size,
            modified: Some((Utc::now() - Duration::days(days_old)).to_rfc3339()),
            category: categories::category(extension),
            is_directory: false,
        }
    }
    fn plan(files: &[FileEntry], groups: &[DuplicateGroup], target: u64, free: u64) -> RescuePlan {
        let valid = files.iter().map(|file| file.path.clone()).collect();
        build(files, groups, &[], target, free, Utc::now(), &valid, 0)
    }

    #[test]
    fn existing_free_space_can_already_satisfy_target() {
        let result = plan(&[], &[], 10 * GB, 20 * GB);
        assert!(result.already_available);
        assert!(result.target_achievable);
        assert!(result.opportunities.is_empty());
    }

    #[test]
    fn zero_opportunities_cannot_fabricate_recovery() {
        let result = plan(&[], &[], 10 * GB, 0);
        assert!(!result.target_achievable);
        assert_eq!(result.estimated_recoverable_bytes, 0);
    }

    #[test]
    fn duplicates_always_retain_one_copy() {
        let files = vec![
            file("D:\\a.bin", 5 * GB, "bin", 1),
            file("D:\\b.bin", 5 * GB, "bin", 1),
            file("D:\\c.bin", 5 * GB, "bin", 1),
        ];
        let group = DuplicateGroup {
            hash: "verified".into(),
            size: 5 * GB,
            files: files.clone(),
            recoverable_size: 10 * GB,
        };
        let result = plan(&files, &[group], 100 * GB, 0);
        assert_eq!(result.total_reviewable_bytes, 10 * GB);
        assert_eq!(result.opportunities[0].estimated_bytes, 10 * GB);
    }

    #[test]
    fn overlapping_duplicate_old_archive_is_counted_once() {
        let files = vec![
            file("D:\\a.zip", 8 * GB, "zip", 500),
            file("D:\\b.zip", 8 * GB, "zip", 500),
        ];
        let group = DuplicateGroup {
            hash: "verified".into(),
            size: 8 * GB,
            files: files.clone(),
            recoverable_size: 8 * GB,
        };
        let result = plan(&files, &[group], 100 * GB, 0);
        assert_eq!(result.total_reviewable_bytes, 8 * GB);
        let duplicate = result
            .opportunities
            .iter()
            .find(|item| item.id == "exact_duplicates")
            .unwrap();
        assert_eq!(duplicate.estimated_bytes, 8 * GB);
    }

    #[test]
    fn classifies_old_files_archives_installers_and_large_files() {
        let files = vec![
            file("D:\\old.bin", 2 * GB, "bin", 200),
            file("D:\\backup.zip", 700 * MB, "zip", 2),
            file("D:\\Downloads\\tool-setup.exe", 200 * MB, "exe", 2),
            file("D:\\movie.mkv", 3 * GB, "mkv", 2),
        ];
        let result = plan(&files, &[], 100 * GB, 0);
        let ids: HashSet<_> = result
            .opportunities
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert!(ids.contains("old_large_files"));
        assert!(ids.contains("large_archives"));
        assert!(ids.contains("installer_files"));
        assert!(ids.contains("large_files"));
    }

    #[test]
    fn arbitrary_executable_is_not_called_an_installer() {
        let files = vec![file("D:\\Downloads\\game.exe", 200 * MB, "exe", 2)];
        let result = plan(&files, &[], 10 * GB, 0);
        assert!(result
            .opportunities
            .iter()
            .all(|item| item.id != "installer_files"));
    }

    #[test]
    fn protected_paths_and_stale_items_are_excluded() {
        let files = vec![file("C:\\Windows\\large.iso", 5 * GB, "iso", 500)];
        let valid = HashSet::new();
        let result = build(&files, &[], &[], 10 * GB, 0, Utc::now(), &valid, 1);
        assert_eq!(result.total_reviewable_bytes, 0);
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn plan_stops_after_target_is_covered() {
        let files = vec![
            file("D:\\one.bin", 6 * GB, "bin", 1),
            file("D:\\two.bin", 5 * GB, "bin", 1),
            file("D:\\three.bin", 4 * GB, "bin", 1),
        ];
        let result = plan(&files, &[], 10 * GB, 0);
        assert!(result.target_achievable);
        assert_eq!(result.estimated_recoverable_bytes, 11 * GB);
        assert_eq!(result.opportunities[0].item_count, 2);
        assert_eq!(result.total_reviewable_bytes, 15 * GB);
    }

    #[test]
    fn partial_target_fulfillment_reports_unachievable() {
        let files = vec![file("D:\\one.bin", 6 * GB, "bin", 1)];
        let result = plan(&files, &[], 10 * GB, 0);
        assert!(!result.target_achievable);
        assert_eq!(result.estimated_recoverable_bytes, 6 * GB);
        assert_eq!(result.confidence, "LOW");
    }

    #[test]
    fn low_risk_duplicates_are_prioritized_before_review_items() {
        let files = vec![
            file("D:\\a.bin", 6 * GB, "bin", 1),
            file("D:\\b.bin", 6 * GB, "bin", 1),
            file("D:\\movie.mkv", 20 * GB, "mkv", 1),
        ];
        let group = DuplicateGroup {
            hash: "verified".into(),
            size: 6 * GB,
            files: files[..2].to_vec(),
            recoverable_size: 6 * GB,
        };
        let result = plan(&files, &[group], 5 * GB, 0);
        assert_eq!(result.opportunities.len(), 1);
        assert_eq!(result.opportunities[0].risk, RescueRisk::Low);
    }

    #[test]
    fn saturates_for_extreme_file_sizes() {
        let files = vec![
            file("D:\\a.bin", u64::MAX, "bin", 1),
            file("D:\\b.bin", u64::MAX, "bin", 1),
            file("D:\\c.bin", u64::MAX, "bin", 1),
        ];
        let group = DuplicateGroup {
            hash: "verified".into(),
            size: u64::MAX,
            files: files.clone(),
            recoverable_size: u64::MAX,
        };
        assert_eq!(
            plan(&files, &[group], u64::MAX, 0).total_reviewable_bytes,
            u64::MAX
        );
    }
}
