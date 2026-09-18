export type Category = "Documents"|"Images"|"Videos"|"Audio"|"Archives"|"Applications"|"Code"|"System"|"Temporary"|"Other";
export interface FileEntry { path:string; name:string; extension:string; size:number; modified:string|null; category:Category; is_directory:boolean }
export interface CategoryTotal { category:Category; size:number; files:number }
export interface ScanSummary { root:string; total_size:number; volume_root:string; volume_capacity:number; volume_used:number; volume_free:number; file_count:number; directory_count:number; category_totals:CategoryTotal[]; largest_files:FileEntry[]; largest_directories:FileEntry[]; scanned_at:string; cancelled:boolean; inaccessible_count:number; timeline_warning:string|null }
export interface ScanProgress { phase:string; files_scanned:number; directories_scanned:number; bytes_scanned:number; inaccessible_count:number; current_path:string; last_progress_unix_ms:number; activity:string; stalled:boolean }
export interface DuplicateProgress { phase:string; processed_files:number; total_files:number; bytes_hashed:number; total_bytes:number; current_path:string }
export interface DuplicateGroup { hash:string; size:number; files:FileEntry[]; recoverable_size:number }
export type RescueRisk="LOW"|"REVIEW_REQUIRED";
export interface RescueOpportunity { id:string; category:string; estimated_bytes:number; risk:RescueRisk; reason:string; item_count:number; action_type:string; item_paths:string[] }
export interface RescuePlan { target_bytes:number; current_free_bytes:number; additional_bytes_needed:number; estimated_recoverable_bytes:number; total_reviewable_bytes:number; target_achievable:boolean; already_available:boolean; opportunities:RescueOpportunity[]; warnings:string[]; confidence:"HIGH"|"MEDIUM"|"LOW" }
export interface TimelinePoint { timestamp:string; used_bytes:number; free_bytes:number; scanned_bytes:number }
export interface TimelineDelta { name:string; path:string|null; delta_bytes:number }
export interface TimelineFileChange { path:string; name:string; kind:"NEW"|"REMOVED"|"GROWN"|"SHRUNK"; previous_size:number|null; current_size:number|null; delta_bytes:number }
export interface StorageChangeReport { previous_timestamp:string; current_timestamp:string; used_space_delta:number; free_space_delta:number; scanned_size_delta:number; category_deltas:TimelineDelta[]; developer_category_deltas?:TimelineDelta[]; directory_deltas:TimelineDelta[]; file_changes:TimelineFileChange[]; spacepilot_recovered_bytes:number; summary:string; warnings:string[] }
export interface TimelineResponse { snapshots:TimelinePoint[]; report:StorageChangeReport|null; first_snapshot:boolean; insufficient_range:boolean; scope_root:string|null; warnings:string[]; history_bytes:number }
export interface TimelineStatus { status:"saving"|"saved"|"warning"; message:string }
export interface Recommendation { file:FileEntry; reason:string; risk:"Safe"|"Review"|"Potentially risky" }
export interface TreeNode { path:string; name:string; size:number; is_directory:boolean }
export type DeveloperConfidence="HIGH"|"MEDIUM"|"LOW";
export type DeveloperRisk="REVIEW_REQUIRED"|"HIGH_MANAGED_EXTERNALLY";
export interface DeveloperStorageFinding { id:string; category:string; subtype:string; path:string; name:string; size_bytes:number; item_count:number; modified:string|null; confidence:DeveloperConfidence; risk:DeveloperRisk; explanation:string; review_action:string; source:string; reviewable:boolean }
export interface DeveloperCategoryTotal { category:string; size_bytes:number; finding_count:number }
export interface DeveloperStorageReport { total_bytes:number; potential_review_bytes:number; findings:DeveloperStorageFinding[]; categories:DeveloperCategoryTotal[]; summary:string; warnings:string[] }
export type AiActionType="OPEN_SPACE_RESCUE"|"OPEN_DUPLICATES"|"OPEN_LARGE_FILES"|"OPEN_STORAGE"|"OPEN_TIMELINE"|"OPEN_DEVELOPER_STORAGE";
export interface AiAction { type:AiActionType; target_id:string|null }
export interface AiRecommendation { title:string; explanation:string; estimated_bytes:number|null; risk:string; source:string }
export interface AiCopilotResponse { summary:string; facts:string[]; recommendations:AiRecommendation[]; warnings:string[]; actions:AiAction[]; provider:string; local:boolean }
export interface AiProviderStatus { provider:string; connected:boolean; selected_model:string; usage_date:string; usage_count:number; usage_limit:number }
export interface FileInspectorMatch { path:string; name:string; extension:string; size:number; modified:string|null; category:Category; label:string }
export interface FileInspectionResult { path:string; name:string; extension:string; size:number; modified:string|null; category:Category; extracted_characters:number; truncated:boolean; local_summary:string; warnings:string[]; cloud_allowed:boolean; cloud_block_reason:string|null }
