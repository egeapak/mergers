//! Output system for non-interactive merge mode.
//!
//! This module provides structured output events and formatters for different
//! output formats (text, JSON, NDJSON). It enables consistent progress reporting
//! and final summaries across all output modes.

mod events;
mod format;

pub use events::{
    ConflictInfo, PostMergeStatus, ProgressEvent, StatusInfo, SummaryInfo, SummaryItem,
};
pub use format::{OutputFormatter, OutputWriter};
