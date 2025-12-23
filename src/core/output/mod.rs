//! Output system for non-interactive merge mode.
//!
//! This module provides structured output events and formatters for different
//! output formats (text, JSON, NDJSON). It enables consistent progress reporting
//! and final summaries across all output modes.

mod events;
mod format;

pub use events::{
    ConflictInfo, ItemStatus, PostMergeStatus, ProgressEvent, ProgressSummary, StatusInfo,
    SummaryCounts, SummaryInfo, SummaryItem, SummaryResult,
};
pub use format::{OutputFormatter, OutputWriter};
