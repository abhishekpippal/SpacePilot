use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub modified: Option<String>,
    pub category: String,
    pub is_directory: bool,
}
#[derive(Clone, Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub size: u64,
    pub files: u64,
}
#[derive(Clone, Serialize)]
pub struct ScanSummary {
    pub root: String,
    pub total_size: u64,
    pub volume_root: String,
    pub volume_capacity: u64,
    pub volume_used: u64,
    pub volume_free: u64,
    pub file_count: u64,
    pub directory_count: u64,
    pub category_totals: Vec<CategoryTotal>,
    pub largest_files: Vec<FileEntry>,
    pub largest_directories: Vec<FileEntry>,
    pub scanned_at: String,
    pub cancelled: bool,
    pub inaccessible_count: u64,
    pub timeline_warning: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct TreeNode {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
}
#[derive(Clone, Serialize, Default)]
pub struct ScanProgress {
    pub phase: String,
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub bytes_scanned: u64,
    pub inaccessible_count: u64,
    pub current_path: String,
    pub last_progress_unix_ms: u64,
    pub activity: String,
    pub stalled: bool,
}
#[derive(Clone, Serialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub files: Vec<FileEntry>,
    pub recoverable_size: u64,
}
#[derive(Clone, Serialize)]
pub struct DuplicateProgress {
    pub phase: String,
    pub processed_files: u64,
    pub total_files: u64,
    pub bytes_hashed: u64,
    pub total_bytes: u64,
    pub current_path: String,
}
#[derive(Serialize)]
pub struct Recommendation {
    pub file: FileEntry,
    pub reason: String,
    pub risk: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RescueRisk {
    Low,
    ReviewRequired,
}

#[derive(Clone, Serialize)]
pub struct RescueOpportunity {
    pub id: String,
    pub category: String,
    pub estimated_bytes: u64,
    pub risk: RescueRisk,
    pub reason: String,
    pub item_count: u64,
    pub action_type: String,
    pub item_paths: Vec<String>,
}

#[derive(Serialize)]
pub struct RescuePlan {
    pub target_bytes: u64,
    pub current_free_bytes: u64,
    pub additional_bytes_needed: u64,
    pub estimated_recoverable_bytes: u64,
    pub total_reviewable_bytes: u64,
    pub target_achievable: bool,
    pub already_available: bool,
    pub opportunities: Vec<RescueOpportunity>,
    pub warnings: Vec<String>,
    pub confidence: String,
}

#[derive(Clone, Serialize)]
pub struct TimelinePoint {
    pub timestamp: String,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub scanned_bytes: u64,
}

#[derive(Serialize)]
pub struct TimelineDelta {
    pub name: String,
    pub path: Option<String>,
    pub delta_bytes: i64,
}

#[derive(Serialize)]
pub struct TimelineFileChange {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub previous_size: Option<u64>,
    pub current_size: Option<u64>,
    pub delta_bytes: i64,
}

#[derive(Serialize)]
pub struct StorageChangeReport {
    pub previous_timestamp: String,
    pub current_timestamp: String,
    pub used_space_delta: i64,
    pub free_space_delta: i64,
    pub scanned_size_delta: i64,
    pub category_deltas: Vec<TimelineDelta>,
    pub developer_category_deltas: Vec<TimelineDelta>,
    pub directory_deltas: Vec<TimelineDelta>,
    pub file_changes: Vec<TimelineFileChange>,
    pub spacepilot_recovered_bytes: u64,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct TimelineResponse {
    pub snapshots: Vec<TimelinePoint>,
    pub report: Option<StorageChangeReport>,
    pub first_snapshot: bool,
    pub insufficient_range: bool,
    pub scope_root: Option<String>,
    pub warnings: Vec<String>,
    pub history_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(dead_code)]
pub enum DeveloperConfidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeveloperRisk {
    ReviewRequired,
    HighManagedExternally,
}

#[derive(Clone, Serialize)]
pub struct DeveloperStorageFinding {
    pub id: String,
    pub category: String,
    pub subtype: String,
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub item_count: u64,
    pub modified: Option<String>,
    pub confidence: DeveloperConfidence,
    pub risk: DeveloperRisk,
    pub explanation: String,
    pub review_action: String,
    pub source: String,
    pub reviewable: bool,
}

#[derive(Clone, Serialize)]
pub struct DeveloperCategoryTotal {
    pub category: String,
    pub size_bytes: u64,
    pub finding_count: u64,
}

#[derive(Clone, Serialize)]
pub struct DeveloperStorageReport {
    pub total_bytes: u64,
    pub potential_review_bytes: u64,
    pub findings: Vec<DeveloperStorageFinding>,
    pub categories: Vec<DeveloperCategoryTotal>,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiActionType {
    OpenSpaceRescue,
    OpenDuplicates,
    OpenLargeFiles,
    OpenStorage,
    OpenTimeline,
    OpenDeveloperStorage,
}

#[derive(Clone, Serialize, serde::Deserialize)]
pub struct AiAction {
    #[serde(rename = "type")]
    pub action_type: AiActionType,
    pub target_id: Option<String>,
}

#[derive(Clone, Serialize, serde::Deserialize)]
pub struct AiRecommendation {
    pub title: String,
    pub explanation: String,
    pub estimated_bytes: Option<u64>,
    pub risk: String,
    pub source: String,
}

#[derive(Clone, Serialize, serde::Deserialize)]
pub struct AiCopilotResponse {
    pub summary: String,
    pub facts: Vec<String>,
    pub recommendations: Vec<AiRecommendation>,
    pub warnings: Vec<String>,
    pub actions: Vec<AiAction>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub local: bool,
}

#[derive(Clone, Serialize)]
pub struct FileInspectorMatch {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub modified: Option<String>,
    pub category: String,
    pub label: String,
}

#[derive(Clone, Serialize)]
pub struct FileInspectionResult {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub modified: Option<String>,
    pub category: String,
    pub extracted_characters: usize,
    pub truncated: bool,
    pub local_summary: String,
    pub warnings: Vec<String>,
    pub cloud_allowed: bool,
    pub cloud_block_reason: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct AiProviderStatus {
    pub provider: String,
    pub connected: bool,
    pub selected_model: String,
    pub usage_date: String,
    pub usage_count: u32,
    pub usage_limit: u32,
}
