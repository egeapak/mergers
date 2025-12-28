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
//! - [`dependency_analysis`] - Analyzing file-level dependencies between PRs
//! - [`cherry_pick`] - Cherry-picking commits with conflict handling
//! - [`post_merge`] - Tagging PRs and updating work items

pub mod cherry_pick;
pub mod data_loading;
pub mod dependency_analysis;
pub mod post_merge;
pub mod pr_selection;

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
pub use post_merge::{
    PostMergeConfig, PostMergeOperation, PostMergeProgress, PostMergeTask, PostMergeTaskResult,
};
pub use pr_selection::{
    filter_prs_by_work_item_states, parse_work_item_states, select_prs_by_work_item_states,
};
