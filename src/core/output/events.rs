//! Progress events for non-interactive merge mode.
//!
//! These events represent the different stages and outcomes of a merge operation,
//! designed to be serializable for JSON/NDJSON output and renderable for text output.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Progress events emitted during merge operations.
///
/// Each variant represents a distinct stage or outcome that should be
/// communicated to the user or consuming system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ProgressEvent {
    /// Merge operation is starting.
    Start {
        /// Total number of PRs to process.
        total_prs: usize,
        /// Version/tag being created.
        version: String,
        /// Target branch for the merge.
        target_branch: String,
    },

    /// Starting to cherry-pick a specific commit.
    CherryPickStart {
        /// PR ID being processed.
        pr_id: i32,
        /// Commit ID being cherry-picked.
        commit_id: String,
        /// Current index (0-based).
        index: usize,
        /// Total number of commits to process.
        total: usize,
    },

    /// Cherry-pick completed successfully.
    CherryPickSuccess {
        /// PR ID that was processed.
        pr_id: i32,
        /// Commit ID that was cherry-picked.
        commit_id: String,
    },

    /// Cherry-pick resulted in conflicts.
    CherryPickConflict {
        /// PR ID with conflicts.
        pr_id: i32,
        /// List of conflicted files.
        conflicted_files: Vec<String>,
        /// Path to the repository for conflict resolution.
        repo_path: PathBuf,
    },

    /// Cherry-pick failed with an error.
    CherryPickFailed {
        /// PR ID that failed.
        pr_id: i32,
        /// Error message.
        error: String,
    },

    /// Cherry-pick was skipped.
    CherryPickSkipped {
        /// PR ID that was skipped.
        pr_id: i32,
        /// Optional reason for skipping.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Dependency analysis is starting.
    DependencyAnalysisStart {
        /// Number of PRs to analyze.
        pr_count: usize,
    },

    /// Dependency analysis completed.
    DependencyAnalysisComplete {
        /// Number of PRs with independent relationships.
        independent: usize,
        /// Number of PRs with partial dependencies.
        partial: usize,
        /// Number of PRs with full dependencies.
        dependent: usize,
    },

    /// A dependency warning was detected.
    DependencyWarning {
        /// The selected PR that has the dependency.
        selected_pr_id: i32,
        /// Title of the selected PR.
        selected_pr_title: String,
        /// The unselected PR that is depended upon.
        unselected_pr_id: i32,
        /// Title of the unselected PR.
        unselected_pr_title: String,
        /// Whether this is a critical (line-level overlap) dependency.
        is_critical: bool,
        /// List of shared files.
        shared_files: Vec<String>,
    },

    /// Post-merge operations are starting.
    PostMergeStart {
        /// Total number of tasks to execute.
        task_count: usize,
    },

    /// Progress on a post-merge task.
    PostMergeProgress {
        /// Type of task (e.g., "tag_pr", "update_work_item").
        task_type: String,
        /// Target ID (PR ID or work item ID).
        target_id: i32,
        /// Status of the task.
        status: PostMergeStatus,
    },

    /// Merge operation completed.
    Complete {
        /// Number of successful cherry-picks.
        successful: usize,
        /// Number of failed cherry-picks.
        failed: usize,
        /// Number of skipped cherry-picks.
        skipped: usize,
    },

    /// Status query response.
    Status(Box<StatusInfo>),

    /// Abort operation completed.
    Aborted {
        /// Whether the abort was successful.
        success: bool,
        /// Optional message.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// Error event for general errors.
    Error {
        /// Error message.
        message: String,
        /// Optional error code.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },

    /// User-defined hook is starting.
    HookStart {
        /// The trigger point (e.g., "post_checkout", "post_merge").
        trigger: String,
        /// Number of commands to run.
        command_count: usize,
    },

    /// A hook command is starting.
    HookCommandStart {
        /// The trigger point.
        trigger: String,
        /// The command being run.
        command: String,
        /// Zero-based index of the command.
        index: usize,
    },

    /// A hook command completed.
    HookCommandComplete {
        /// The trigger point.
        trigger: String,
        /// The command that ran.
        command: String,
        /// Whether it succeeded.
        success: bool,
        /// Zero-based index of the command.
        index: usize,
    },

    /// All hooks for a trigger completed.
    HookComplete {
        /// The trigger point.
        trigger: String,
        /// Whether all commands succeeded.
        all_succeeded: bool,
    },

    /// A hook failed and the operation is stopping.
    HookFailed {
        /// The trigger point.
        trigger: String,
        /// The command that failed.
        command: String,
        /// Error output from the command.
        error: String,
    },
}

/// Status of a post-merge task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PostMergeStatus {
    /// Task is pending.
    Pending,
    /// Task completed successfully.
    Success,
    /// Task failed.
    Failed {
        /// Error message.
        error: String,
    },
    /// Task was skipped.
    Skipped,
}

impl std::fmt::Display for PostMergeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostMergeStatus::Pending => write!(f, "pending"),
            PostMergeStatus::Success => write!(f, "success"),
            PostMergeStatus::Failed { error } => write!(f, "failed: {}", error),
            PostMergeStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// Detailed information about conflicts for output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConflictInfo {
    /// PR ID with conflicts.
    pub pr_id: i32,
    /// PR title.
    pub pr_title: String,
    /// Commit ID being cherry-picked.
    pub commit_id: String,
    /// List of conflicted files.
    pub conflicted_files: Vec<String>,
    /// Path to the repository for resolution.
    pub repo_path: PathBuf,
    /// Instructions for resolution.
    pub resolution_instructions: Vec<String>,
}

impl ConflictInfo {
    /// Creates a new ConflictInfo with default resolution instructions.
    pub fn new(
        pr_id: i32,
        pr_title: String,
        commit_id: String,
        conflicted_files: Vec<String>,
        repo_path: PathBuf,
    ) -> Self {
        let instructions = vec![
            format!("1. Navigate to: {}", repo_path.display()),
            "2. Resolve conflicts in the listed files".to_string(),
            "3. Stage resolved files: git add <files>".to_string(),
            "4. Run: mergers merge continue".to_string(),
        ];

        Self {
            pr_id,
            pr_title,
            commit_id,
            conflicted_files,
            repo_path,
            resolution_instructions: instructions,
        }
    }
}

/// Status information for the current merge state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusInfo {
    /// Current phase of the merge.
    pub phase: String,
    /// Overall status.
    pub status: String,
    /// Version being created.
    pub version: String,
    /// Target branch.
    pub target_branch: String,
    /// Repository path.
    pub repo_path: PathBuf,
    /// Cherry-pick progress summary.
    pub progress: ProgressSummary,
    /// Conflict info if in conflict state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictInfo>,
    /// List of items with their statuses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<SummaryItem>>,
}

/// Summary of cherry-pick progress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressSummary {
    /// Total number of items.
    pub total: usize,
    /// Number of completed items.
    pub completed: usize,
    /// Number of pending items.
    pub pending: usize,
    /// Current index being processed.
    pub current_index: usize,
}

/// Summary information for final output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryInfo {
    /// Overall result status.
    pub result: SummaryResult,
    /// Version that was created.
    pub version: String,
    /// Target branch.
    pub target_branch: String,
    /// Counts of different outcomes.
    pub counts: SummaryCounts,
    /// Detailed items (optional, for verbose output).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<SummaryItem>>,
    /// Post-merge task results (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_merge: Option<PostMergeSummary>,
}

/// Overall result of the merge operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryResult {
    /// All items processed successfully.
    Success,
    /// Some items failed but others succeeded.
    PartialSuccess,
    /// All items failed.
    Failed,
    /// Operation was aborted.
    Aborted,
    /// Conflicts need resolution.
    Conflict,
}

impl std::fmt::Display for SummaryResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SummaryResult::Success => write!(f, "success"),
            SummaryResult::PartialSuccess => write!(f, "partial_success"),
            SummaryResult::Failed => write!(f, "failed"),
            SummaryResult::Aborted => write!(f, "aborted"),
            SummaryResult::Conflict => write!(f, "conflict"),
        }
    }
}

/// Counts for the summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryCounts {
    /// Total number of items.
    pub total: usize,
    /// Successfully processed.
    pub successful: usize,
    /// Failed to process.
    pub failed: usize,
    /// Skipped items.
    pub skipped: usize,
    /// Pending items (not yet processed).
    pub pending: usize,
}

impl SummaryCounts {
    /// Creates counts from component values.
    pub fn new(successful: usize, failed: usize, skipped: usize, pending: usize) -> Self {
        Self {
            total: successful + failed + skipped + pending,
            successful,
            failed,
            skipped,
            pending,
        }
    }
}

/// Individual item in a summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryItem {
    /// PR ID.
    pub pr_id: i32,
    /// PR title.
    pub pr_title: String,
    /// Commit ID.
    pub commit_id: String,
    /// Status of the item.
    pub status: ItemStatus,
    /// Error message if failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Status of an individual item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// Pending processing.
    Pending,
    /// Currently being processed.
    InProgress,
    /// Successfully processed.
    Success,
    /// Failed to process.
    Failed,
    /// Skipped.
    Skipped,
    /// Has conflicts.
    Conflict,
}

impl std::fmt::Display for ItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ItemStatus::Pending => write!(f, "pending"),
            ItemStatus::InProgress => write!(f, "in_progress"),
            ItemStatus::Success => write!(f, "success"),
            ItemStatus::Failed => write!(f, "failed"),
            ItemStatus::Skipped => write!(f, "skipped"),
            ItemStatus::Conflict => write!(f, "conflict"),
        }
    }
}

/// Summary of post-merge operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostMergeSummary {
    /// Total tasks executed.
    pub total_tasks: usize,
    /// Successfully completed tasks.
    pub successful: usize,
    /// Failed tasks.
    pub failed: usize,
    /// Individual task results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<PostMergeTaskResult>>,
}

/// Result of a single post-merge task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostMergeTaskResult {
    /// Type of task.
    pub task_type: String,
    /// Target ID.
    pub target_id: i32,
    /// Status.
    pub status: PostMergeStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Progress Event Serialization
    ///
    /// Verifies that progress events serialize correctly to JSON.
    ///
    /// ## Test Scenario
    /// - Creates various event types
    /// - Serializes to JSON
    /// - Verifies the output format
    ///
    /// ## Expected Outcome
    /// - All events serialize with correct structure
    #[test]
    fn test_progress_event_serialization() {
        let event = ProgressEvent::Start {
            total_prs: 5,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"start\""));
        assert!(json.contains("\"total_prs\":5"));
        assert!(json.contains("\"version\":\"v1.0.0\""));
    }

    /// # Cherry-Pick Event Serialization
    ///
    /// Verifies cherry-pick events serialize correctly.
    ///
    /// ## Test Scenario
    /// - Creates cherry-pick start, success, conflict, and failed events
    /// - Serializes each to JSON
    ///
    /// ## Expected Outcome
    /// - Correct event tags and fields in JSON
    #[test]
    fn test_cherry_pick_events_serialization() {
        let start = ProgressEvent::CherryPickStart {
            pr_id: 123,
            commit_id: "abc123".to_string(),
            index: 0,
            total: 5,
        };
        let json = serde_json::to_string(&start).unwrap();
        assert!(json.contains("\"event\":\"cherry_pick_start\""));
        assert!(json.contains("\"pr_id\":123"));

        let success = ProgressEvent::CherryPickSuccess {
            pr_id: 123,
            commit_id: "abc123".to_string(),
        };
        let json = serde_json::to_string(&success).unwrap();
        assert!(json.contains("\"event\":\"cherry_pick_success\""));

        let conflict = ProgressEvent::CherryPickConflict {
            pr_id: 123,
            conflicted_files: vec!["file1.rs".to_string(), "file2.rs".to_string()],
            repo_path: PathBuf::from("/tmp/repo"),
        };
        let json = serde_json::to_string(&conflict).unwrap();
        assert!(json.contains("\"event\":\"cherry_pick_conflict\""));
        assert!(json.contains("\"conflicted_files\""));

        let failed = ProgressEvent::CherryPickFailed {
            pr_id: 123,
            error: "merge failed".to_string(),
        };
        let json = serde_json::to_string(&failed).unwrap();
        assert!(json.contains("\"event\":\"cherry_pick_failed\""));
    }

    /// # Complete Event Serialization
    ///
    /// Verifies the complete event serializes with counts.
    ///
    /// ## Test Scenario
    /// - Creates a complete event with counts
    /// - Serializes to JSON
    ///
    /// ## Expected Outcome
    /// - All count fields present in JSON
    #[test]
    fn test_complete_event_serialization() {
        let event = ProgressEvent::Complete {
            successful: 3,
            failed: 1,
            skipped: 1,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"complete\""));
        assert!(json.contains("\"successful\":3"));
        assert!(json.contains("\"failed\":1"));
        assert!(json.contains("\"skipped\":1"));
    }

    /// # Post-Merge Status Display
    ///
    /// Verifies PostMergeStatus display trait works correctly.
    ///
    /// ## Test Scenario
    /// - Creates different status variants
    /// - Converts to string
    ///
    /// ## Expected Outcome
    /// - Human-readable status strings
    #[test]
    fn test_post_merge_status_display() {
        assert_eq!(PostMergeStatus::Pending.to_string(), "pending");
        assert_eq!(PostMergeStatus::Success.to_string(), "success");
        assert_eq!(PostMergeStatus::Skipped.to_string(), "skipped");
        assert_eq!(
            PostMergeStatus::Failed {
                error: "test error".to_string()
            }
            .to_string(),
            "failed: test error"
        );
    }

    /// # Conflict Info Creation
    ///
    /// Verifies ConflictInfo creates with default instructions.
    ///
    /// ## Test Scenario
    /// - Creates ConflictInfo with basic data
    ///
    /// ## Expected Outcome
    /// - Default resolution instructions are set
    #[test]
    fn test_conflict_info_creation() {
        let info = ConflictInfo::new(
            123,
            "Test PR".to_string(),
            "abc123".to_string(),
            vec!["file1.rs".to_string()],
            PathBuf::from("/tmp/repo"),
        );

        assert_eq!(info.pr_id, 123);
        assert_eq!(info.conflicted_files.len(), 1);
        assert_eq!(info.resolution_instructions.len(), 4);
        assert!(info.resolution_instructions[0].contains("/tmp/repo"));
    }

    /// # Summary Counts Creation
    ///
    /// Verifies SummaryCounts calculates total correctly.
    ///
    /// ## Test Scenario
    /// - Creates counts with component values
    ///
    /// ## Expected Outcome
    /// - Total is sum of all components
    #[test]
    fn test_summary_counts_creation() {
        let counts = SummaryCounts::new(3, 1, 1, 0);
        assert_eq!(counts.total, 5);
        assert_eq!(counts.successful, 3);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.pending, 0);
    }

    /// # Item Status Display
    ///
    /// Verifies ItemStatus display trait.
    ///
    /// ## Test Scenario
    /// - Converts all status variants to strings
    ///
    /// ## Expected Outcome
    /// - Correct string representation for each
    #[test]
    fn test_item_status_display() {
        assert_eq!(ItemStatus::Pending.to_string(), "pending");
        assert_eq!(ItemStatus::InProgress.to_string(), "in_progress");
        assert_eq!(ItemStatus::Success.to_string(), "success");
        assert_eq!(ItemStatus::Failed.to_string(), "failed");
        assert_eq!(ItemStatus::Skipped.to_string(), "skipped");
        assert_eq!(ItemStatus::Conflict.to_string(), "conflict");
    }

    /// # Summary Result Display
    ///
    /// Verifies SummaryResult display trait.
    ///
    /// ## Test Scenario
    /// - Converts all result variants to strings
    ///
    /// ## Expected Outcome
    /// - Correct string representation for each
    #[test]
    fn test_summary_result_display() {
        assert_eq!(SummaryResult::Success.to_string(), "success");
        assert_eq!(SummaryResult::PartialSuccess.to_string(), "partial_success");
        assert_eq!(SummaryResult::Failed.to_string(), "failed");
        assert_eq!(SummaryResult::Aborted.to_string(), "aborted");
        assert_eq!(SummaryResult::Conflict.to_string(), "conflict");
    }

    /// # Event Deserialization Round-Trip
    ///
    /// Verifies events can be serialized and deserialized.
    ///
    /// ## Test Scenario
    /// - Creates event, serializes, deserializes
    ///
    /// ## Expected Outcome
    /// - Deserialized event equals original
    #[test]
    fn test_event_round_trip() {
        let original = ProgressEvent::CherryPickStart {
            pr_id: 456,
            commit_id: "def789".to_string(),
            index: 2,
            total: 10,
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ProgressEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    /// # Skipped Event Optional Reason
    ///
    /// Verifies CherryPickSkipped handles optional reason.
    ///
    /// ## Test Scenario
    /// - Creates skipped events with and without reason
    ///
    /// ## Expected Outcome
    /// - Reason field is omitted when None
    #[test]
    fn test_skipped_event_optional_reason() {
        let without_reason = ProgressEvent::CherryPickSkipped {
            pr_id: 123,
            reason: None,
        };
        let json = serde_json::to_string(&without_reason).unwrap();
        assert!(!json.contains("reason"));

        let with_reason = ProgressEvent::CherryPickSkipped {
            pr_id: 123,
            reason: Some("user requested".to_string()),
        };
        let json = serde_json::to_string(&with_reason).unwrap();
        assert!(json.contains("\"reason\":\"user requested\""));
    }

    /// # All Events Have Event Field
    ///
    /// Verifies all ProgressEvent variants serialize with an "event" tag field.
    ///
    /// ## Test Scenario
    /// - Creates one of each event variant
    /// - Serializes each to JSON
    ///
    /// ## Expected Outcome
    /// - All serialized events contain "event" field
    #[test]
    fn test_event_has_event_field() {
        let events: Vec<ProgressEvent> = vec![
            ProgressEvent::Start {
                total_prs: 1,
                version: "v1".to_string(),
                target_branch: "main".to_string(),
            },
            ProgressEvent::CherryPickStart {
                pr_id: 1,
                commit_id: "abc".to_string(),
                index: 0,
                total: 1,
            },
            ProgressEvent::CherryPickSuccess {
                pr_id: 1,
                commit_id: "abc".to_string(),
            },
            ProgressEvent::CherryPickConflict {
                pr_id: 1,
                conflicted_files: vec![],
                repo_path: PathBuf::from("/tmp"),
            },
            ProgressEvent::CherryPickFailed {
                pr_id: 1,
                error: "error".to_string(),
            },
            ProgressEvent::CherryPickSkipped {
                pr_id: 1,
                reason: None,
            },
            ProgressEvent::PostMergeStart { task_count: 1 },
            ProgressEvent::PostMergeProgress {
                task_type: "tag".to_string(),
                target_id: 1,
                status: PostMergeStatus::Success,
            },
            ProgressEvent::Complete {
                successful: 1,
                failed: 0,
                skipped: 0,
            },
            ProgressEvent::Aborted {
                success: true,
                message: None,
            },
            ProgressEvent::Error {
                message: "err".to_string(),
                code: None,
            },
            ProgressEvent::HookStart {
                trigger: "post_checkout".to_string(),
                command_count: 2,
            },
            ProgressEvent::HookCommandStart {
                trigger: "post_checkout".to_string(),
                command: "npm install".to_string(),
                index: 0,
            },
            ProgressEvent::HookCommandComplete {
                trigger: "post_checkout".to_string(),
                command: "npm install".to_string(),
                success: true,
                index: 0,
            },
            ProgressEvent::HookComplete {
                trigger: "post_checkout".to_string(),
                all_succeeded: true,
            },
            ProgressEvent::HookFailed {
                trigger: "post_merge".to_string(),
                command: "cargo test".to_string(),
                error: "test failed".to_string(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            assert!(
                json.contains("\"event\":"),
                "Event should have 'event' field: {}",
                json
            );
        }
    }
}
