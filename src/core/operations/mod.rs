//! Core operations for merge workflows.
//!
//! This module provides UI-independent implementations of the core operations
//! needed for merge workflows. These operations can be used by both the
//! interactive TUI and non-interactive CLI modes.
//!
//! # Modules
//!
//! - [`data_loading`] - Fetching PRs and work items from Azure DevOps
//! - [`pr_selection`] - Filtering and selecting PRs by work item state
//! - [`work_item_grouping`] - Grouping PRs that share work items
//! - [`dependency_analysis`] - Analyzing file-level dependencies between PRs
//! - [`cherry_pick`] - Cherry-picking commits with conflict handling
//! - [`post_merge`] - Tagging PRs and updating work items
//! - [`hooks`] - User-defined shell command hooks for merge workflows

pub mod cherry_pick;
pub mod data_loading;
pub mod dependency_analysis;
pub mod hooks;
pub mod post_merge;
pub mod pr_selection;
pub mod work_item_grouping;

// Re-export commonly used types
pub use cherry_pick::{
    CherryPickConfig, CherryPickOperation, CherryPickOutcome, CherryPickProgress,
};
pub use data_loading::{
    DataLoadingConfig, DataLoadingOperation, DataLoadingProgress, DataLoadingResult,
};
pub use dependency_analysis::{
    ChangeType, DependencyAnalysisConfig, DependencyAnalysisResult, DependencyAnalyzer,
    DependencyCategory, DependencyWarning, FileChange, LineRange, OverlappingFile, PRBitmapIndex,
    PRDependency, PRDependencyGraph, PRDependencyNode, PRInfo,
};
pub use hooks::{
    HookCommandResult, HookContext, HookExecutionMode, HookExecutor, HookFailureMode, HookOutcome,
    HookProgress, HookResult, HookTrigger, HookTriggerConfig, HooksConfig,
};
pub use post_merge::{
    PostMergeConfig, PostMergeOperation, PostMergeProgress, PostMergeTask, PostMergeTaskResult,
};
pub use pr_selection::{
    filter_prs_by_work_item_states, parse_work_item_states, select_prs_by_work_item_states,
};
pub use work_item_grouping::{
    SelectionWarning, WorkItemPrIndex, check_selection_warning, get_work_item_title,
};
