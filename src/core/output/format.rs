//! Output formatters for different output modes.
//!
//! This module provides formatters for text, JSON, and NDJSON output modes,
//! each implementing the `OutputFormatter` trait for consistent behavior.

use super::events::{
    ConflictInfo, ItemStatus, PostMergeStatus, ProgressEvent, StatusInfo, SummaryInfo,
    SummaryResult,
};
use crate::models::OutputFormat;
use std::io::{self, Write};

/// Trait for formatting and writing output events.
pub trait OutputFormatter {
    /// Writes a progress event to the output.
    fn write_event(&mut self, event: &ProgressEvent) -> io::Result<()>;

    /// Writes a conflict information block.
    fn write_conflict(&mut self, conflict: &ConflictInfo) -> io::Result<()>;

    /// Writes status information.
    fn write_status(&mut self, status: &StatusInfo) -> io::Result<()>;

    /// Writes a final summary.
    fn write_summary(&mut self, summary: &SummaryInfo) -> io::Result<()>;

    /// Flushes any buffered output.
    fn flush(&mut self) -> io::Result<()>;
}

/// Writer that formats output according to the specified format.
pub struct OutputWriter<W: Write> {
    writer: W,
    format: OutputFormat,
    quiet: bool,
    events: Vec<ProgressEvent>,
}

impl<W: Write> OutputWriter<W> {
    /// Creates a new OutputWriter with the specified format.
    pub fn new(writer: W, format: OutputFormat, quiet: bool) -> Self {
        Self {
            writer,
            format,
            quiet,
            events: Vec::new(),
        }
    }

    /// Returns the output format.
    pub fn format(&self) -> &OutputFormat {
        &self.format
    }

    /// Returns whether quiet mode is enabled.
    pub fn is_quiet(&self) -> bool {
        self.quiet
    }

    /// Writes text with optional color support.
    fn write_text(&mut self, text: &str) -> io::Result<()> {
        write!(self.writer, "{}", text)
    }

    /// Writes a line of text.
    fn writeln(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer, "{}", text)
    }

    /// Formats a progress bar string.
    fn format_progress_bar(current: usize, total: usize, width: usize) -> String {
        if total == 0 {
            return format!("[{}]", " ".repeat(width));
        }

        let filled = (current * width) / total;
        let empty = width.saturating_sub(filled);

        format!("[{}{}]", "=".repeat(filled), " ".repeat(empty))
    }

    /// Formats a status indicator with symbol.
    fn status_symbol(status: &ItemStatus) -> &'static str {
        match status {
            ItemStatus::Pending => "○",
            ItemStatus::InProgress => "◐",
            ItemStatus::Success => "✓",
            ItemStatus::Failed => "✗",
            ItemStatus::Skipped => "⊘",
            ItemStatus::Conflict => "⚠",
        }
    }

    /// Formats a post-merge status with symbol.
    fn post_merge_status_symbol(status: &PostMergeStatus) -> &'static str {
        match status {
            PostMergeStatus::Pending => "○",
            PostMergeStatus::Success => "✓",
            PostMergeStatus::Failed { .. } => "✗",
            PostMergeStatus::Skipped => "⊘",
        }
    }
}

impl<W: Write> OutputFormatter for OutputWriter<W> {
    fn write_event(&mut self, event: &ProgressEvent) -> io::Result<()> {
        match self.format {
            OutputFormat::Text => {
                if self.quiet {
                    // In quiet mode, only show errors and conflicts
                    match event {
                        ProgressEvent::CherryPickConflict { .. }
                        | ProgressEvent::CherryPickFailed { .. }
                        | ProgressEvent::Error { .. } => {
                            self.write_text_event(event)?;
                        }
                        _ => {}
                    }
                } else {
                    self.write_text_event(event)?;
                }
            }
            OutputFormat::Json => {
                // Buffer events for final summary
                self.events.push(event.clone());
            }
            OutputFormat::Ndjson => {
                // Write each event as a JSON line
                let json = serde_json::to_string(event).map_err(io::Error::other)?;
                self.writeln(&json)?;
            }
        }
        Ok(())
    }

    fn write_conflict(&mut self, conflict: &ConflictInfo) -> io::Result<()> {
        match self.format {
            OutputFormat::Text => {
                self.writeln("")?;
                self.writeln("╔════════════════════════════════════════════════════════════╗")?;
                self.writeln("║                    CONFLICT DETECTED                        ║")?;
                self.writeln("╚════════════════════════════════════════════════════════════╝")?;
                self.writeln("")?;
                self.writeln(&format!("PR #{}: {}", conflict.pr_id, conflict.pr_title))?;
                self.writeln(&format!("Commit: {}", conflict.commit_id))?;
                self.writeln("")?;
                self.writeln("Conflicted files:")?;
                for file in &conflict.conflicted_files {
                    self.writeln(&format!("  • {}", file))?;
                }
                self.writeln("")?;
                self.writeln("To resolve:")?;
                for instruction in &conflict.resolution_instructions {
                    self.writeln(&format!("  {}", instruction))?;
                }
                self.writeln("")?;
            }
            OutputFormat::Json => {
                // Include in final summary
            }
            OutputFormat::Ndjson => {
                let event = ProgressEvent::CherryPickConflict {
                    pr_id: conflict.pr_id,
                    conflicted_files: conflict.conflicted_files.clone(),
                    repo_path: conflict.repo_path.clone(),
                };
                let json = serde_json::to_string(&event).map_err(io::Error::other)?;
                self.writeln(&json)?;
            }
        }
        Ok(())
    }

    fn write_status(&mut self, status: &StatusInfo) -> io::Result<()> {
        match self.format {
            OutputFormat::Text => {
                self.writeln("")?;
                self.writeln("═══════════════════════════════════════════════════════════")?;
                self.writeln("                      MERGE STATUS                          ")?;
                self.writeln("═══════════════════════════════════════════════════════════")?;
                self.writeln("")?;
                self.writeln(&format!("Version:       {}", status.version))?;
                self.writeln(&format!("Target Branch: {}", status.target_branch))?;
                self.writeln(&format!("Phase:         {}", status.phase))?;
                self.writeln(&format!("Status:        {}", status.status))?;
                self.writeln(&format!("Repository:    {}", status.repo_path.display()))?;
                self.writeln("")?;
                self.writeln("Progress:")?;
                let bar =
                    Self::format_progress_bar(status.progress.completed, status.progress.total, 40);
                self.writeln(&format!(
                    "  {} {}/{}",
                    bar, status.progress.completed, status.progress.total
                ))?;
                self.writeln("")?;

                if let Some(conflict) = &status.conflict {
                    self.writeln("Current Conflict:")?;
                    self.writeln(&format!("  PR #{}: {}", conflict.pr_id, conflict.pr_title))?;
                    self.writeln("  Files:")?;
                    for file in &conflict.conflicted_files {
                        self.writeln(&format!("    • {}", file))?;
                    }
                    self.writeln("")?;
                }

                if let Some(items) = &status.items {
                    self.writeln("Items:")?;
                    for item in items {
                        let symbol = Self::status_symbol(&item.status);
                        self.writeln(&format!(
                            "  {} PR #{}: {} [{}]",
                            symbol,
                            item.pr_id,
                            truncate_string(&item.pr_title, 40),
                            item.status
                        ))?;
                    }
                }
                self.writeln("")?;
            }
            OutputFormat::Json | OutputFormat::Ndjson => {
                let json = serde_json::to_string_pretty(status).map_err(io::Error::other)?;
                self.writeln(&json)?;
            }
        }
        Ok(())
    }

    fn write_summary(&mut self, summary: &SummaryInfo) -> io::Result<()> {
        match self.format {
            OutputFormat::Text => {
                self.writeln("")?;
                let result_line = match summary.result {
                    SummaryResult::Success => "SUCCESS",
                    SummaryResult::PartialSuccess => "PARTIAL SUCCESS",
                    SummaryResult::Failed => "FAILED",
                    SummaryResult::Aborted => "ABORTED",
                    SummaryResult::Conflict => "CONFLICT",
                };
                self.writeln("═══════════════════════════════════════════════════════════")?;
                self.writeln(&format!(
                    "                      {}                          ",
                    result_line
                ))?;
                self.writeln("═══════════════════════════════════════════════════════════")?;
                self.writeln("")?;
                self.writeln(&format!("Version:       {}", summary.version))?;
                self.writeln(&format!("Target Branch: {}", summary.target_branch))?;
                self.writeln("")?;
                self.writeln("Results:")?;
                self.writeln(&format!("  ✓ Successful: {}", summary.counts.successful))?;
                self.writeln(&format!("  ✗ Failed:     {}", summary.counts.failed))?;
                self.writeln(&format!("  ⊘ Skipped:    {}", summary.counts.skipped))?;
                self.writeln(&format!("  ○ Pending:    {}", summary.counts.pending))?;
                self.writeln("  ─────────────────")?;
                self.writeln(&format!("    Total:      {}", summary.counts.total))?;
                self.writeln("")?;

                if let Some(post_merge) = &summary.post_merge {
                    self.writeln("Post-merge tasks:")?;
                    self.writeln(&format!("  ✓ Successful: {}", post_merge.successful))?;
                    self.writeln(&format!("  ✗ Failed:     {}", post_merge.failed))?;
                    self.writeln("")?;
                }
            }
            OutputFormat::Json => {
                // Write the full summary as JSON
                let output = serde_json::json!({
                    "summary": summary,
                    "events": self.events
                });
                let json = serde_json::to_string_pretty(&output).map_err(io::Error::other)?;
                self.writeln(&json)?;
            }
            OutputFormat::Ndjson => {
                // Write summary as final line
                let json = serde_json::to_string(summary).map_err(io::Error::other)?;
                self.writeln(&json)?;
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<W: Write> OutputWriter<W> {
    /// Writes a text-formatted event.
    fn write_text_event(&mut self, event: &ProgressEvent) -> io::Result<()> {
        match event {
            ProgressEvent::Start {
                total_prs,
                version,
                target_branch,
            } => {
                self.writeln("")?;
                self.writeln(&format!(
                    "Starting merge: {} → {} ({} PRs)",
                    version, target_branch, total_prs
                ))?;
                self.writeln("")?;
            }
            ProgressEvent::CherryPickStart {
                pr_id,
                index,
                total,
                ..
            } => {
                let bar = Self::format_progress_bar(*index, *total, 20);
                self.write_text(&format!(
                    "\r{} [{}/{}] Processing PR #{}...",
                    bar,
                    index + 1,
                    total,
                    pr_id
                ))?;
                self.writer.flush()?;
            }
            ProgressEvent::CherryPickSuccess { pr_id, .. } => {
                self.writeln(&format!(" ✓ PR #{} applied", pr_id))?;
            }
            ProgressEvent::CherryPickConflict {
                pr_id,
                conflicted_files,
                repo_path,
            } => {
                self.writeln("")?;
                self.writeln(&format!(" ⚠ PR #{} has conflicts:", pr_id))?;
                for file in conflicted_files {
                    self.writeln(&format!("   • {}", file))?;
                }
                self.writeln(&format!("   Repository: {}", repo_path.display()))?;
            }
            ProgressEvent::CherryPickFailed { pr_id, error } => {
                self.writeln(&format!(" ✗ PR #{} failed: {}", pr_id, error))?;
            }
            ProgressEvent::CherryPickSkipped { pr_id, reason } => {
                let reason_str = reason
                    .as_ref()
                    .map(|r| format!(" ({})", r))
                    .unwrap_or_default();
                self.writeln(&format!(" ⊘ PR #{} skipped{}", pr_id, reason_str))?;
            }
            ProgressEvent::PostMergeStart { task_count } => {
                self.writeln("")?;
                self.writeln(&format!("Running {} post-merge tasks...", task_count))?;
            }
            ProgressEvent::PostMergeProgress {
                task_type,
                target_id,
                status,
            } => {
                let symbol = Self::post_merge_status_symbol(status);
                self.writeln(&format!(
                    "  {} {}: {} #{}",
                    symbol, status, task_type, target_id
                ))?;
            }
            ProgressEvent::Complete {
                successful,
                failed,
                skipped,
            } => {
                self.writeln("")?;
                self.writeln(&format!(
                    "Complete: {} successful, {} failed, {} skipped",
                    successful, failed, skipped
                ))?;
            }
            ProgressEvent::Status(status) => {
                self.write_status(status)?;
            }
            ProgressEvent::Aborted { success, message } => {
                if *success {
                    self.writeln("Merge operation aborted successfully.")?;
                } else {
                    self.writeln("Failed to abort merge operation.")?;
                }
                if let Some(msg) = message {
                    self.writeln(&format!("  {}", msg))?;
                }
            }
            ProgressEvent::Error { message, code } => {
                let code_str = code
                    .as_ref()
                    .map(|c| format!(" [{}]", c))
                    .unwrap_or_default();
                self.writeln(&format!("Error{}: {}", code_str, message))?;
            }
        }
        Ok(())
    }
}

/// Truncates a string to a maximum length, adding ellipsis if needed.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        "...".to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// # Text Output Start Event
    ///
    /// Verifies text formatter handles start event.
    ///
    /// ## Test Scenario
    /// - Creates OutputWriter with text format
    /// - Writes start event
    ///
    /// ## Expected Outcome
    /// - Output contains version, branch, and PR count
    #[test]
    fn test_text_output_start_event() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        let event = ProgressEvent::Start {
            total_prs: 5,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
        };

        writer.write_event(&event).unwrap();
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains("v1.0.0"));
        assert!(output.contains("main"));
        assert!(output.contains("5 PRs"));
    }

    /// # NDJSON Output Events
    ///
    /// Verifies NDJSON formatter writes one JSON per line.
    ///
    /// ## Test Scenario
    /// - Creates OutputWriter with NDJSON format
    /// - Writes multiple events
    ///
    /// ## Expected Outcome
    /// - Each event is on its own line
    /// - Each line is valid JSON
    #[test]
    fn test_ndjson_output_events() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Ndjson, false);

        writer
            .write_event(&ProgressEvent::Start {
                total_prs: 3,
                version: "v1.0.0".to_string(),
                target_branch: "main".to_string(),
            })
            .unwrap();

        writer
            .write_event(&ProgressEvent::CherryPickSuccess {
                pr_id: 123,
                commit_id: "abc".to_string(),
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(serde_json::from_str::<ProgressEvent>(lines[0]).is_ok());
        assert!(serde_json::from_str::<ProgressEvent>(lines[1]).is_ok());
    }

    /// # JSON Output Buffering
    ///
    /// Verifies JSON formatter buffers events for final summary.
    ///
    /// ## Test Scenario
    /// - Creates OutputWriter with JSON format
    /// - Writes events then summary
    ///
    /// ## Expected Outcome
    /// - Summary includes buffered events
    #[test]
    fn test_json_output_buffering() {
        use super::super::events::{SummaryCounts, SummaryInfo, SummaryResult};

        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Json, false);

        // Write events - these should be buffered
        writer
            .write_event(&ProgressEvent::Start {
                total_prs: 2,
                version: "v1.0.0".to_string(),
                target_branch: "main".to_string(),
            })
            .unwrap();

        // Write summary - this should include events
        let summary = SummaryInfo {
            result: SummaryResult::Success,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            counts: SummaryCounts::new(2, 0, 0, 0),
            items: None,
            post_merge: None,
        };

        writer.write_summary(&summary).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"summary\""));
        assert!(output.contains("\"events\""));
    }

    /// # Quiet Mode Suppresses Progress
    ///
    /// Verifies quiet mode only shows errors/conflicts.
    ///
    /// ## Test Scenario
    /// - Creates OutputWriter with quiet mode
    /// - Writes success and error events
    ///
    /// ## Expected Outcome
    /// - Success events are suppressed
    /// - Error events are shown
    #[test]
    fn test_quiet_mode_suppresses_progress() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, true);

        // Success event should be suppressed
        writer
            .write_event(&ProgressEvent::CherryPickSuccess {
                pr_id: 123,
                commit_id: "abc".to_string(),
            })
            .unwrap();

        // Error event should be shown
        writer
            .write_event(&ProgressEvent::Error {
                message: "test error".to_string(),
                code: None,
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(!output.contains("PR #123"));
        assert!(output.contains("test error"));
    }

    /// # Conflict Info Text Formatting
    ///
    /// Verifies conflict info is formatted nicely for text output.
    ///
    /// ## Test Scenario
    /// - Creates ConflictInfo and writes with text formatter
    ///
    /// ## Expected Outcome
    /// - Contains PR info, files, and instructions
    #[test]
    fn test_conflict_info_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        let conflict = ConflictInfo::new(
            123,
            "Test PR".to_string(),
            "abc123".to_string(),
            vec!["file1.rs".to_string(), "file2.rs".to_string()],
            PathBuf::from("/tmp/repo"),
        );

        writer.write_conflict(&conflict).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("CONFLICT DETECTED"));
        assert!(output.contains("PR #123"));
        assert!(output.contains("file1.rs"));
        assert!(output.contains("file2.rs"));
        assert!(output.contains("To resolve"));
    }

    /// # Progress Bar Formatting
    ///
    /// Verifies progress bar renders correctly.
    ///
    /// ## Test Scenario
    /// - Tests various progress values
    ///
    /// ## Expected Outcome
    /// - Bar width is consistent
    /// - Fill level matches progress
    #[test]
    fn test_progress_bar_formatting() {
        let bar0 = OutputWriter::<Vec<u8>>::format_progress_bar(0, 10, 10);
        assert_eq!(bar0, "[          ]");

        let bar5 = OutputWriter::<Vec<u8>>::format_progress_bar(5, 10, 10);
        assert_eq!(bar5, "[=====     ]");

        let bar10 = OutputWriter::<Vec<u8>>::format_progress_bar(10, 10, 10);
        assert_eq!(bar10, "[==========]");

        // Edge case: zero total
        let bar_zero = OutputWriter::<Vec<u8>>::format_progress_bar(0, 0, 10);
        assert_eq!(bar_zero, "[          ]");
    }

    /// # String Truncation
    ///
    /// Verifies string truncation works correctly.
    ///
    /// ## Test Scenario
    /// - Tests strings of various lengths
    ///
    /// ## Expected Outcome
    /// - Short strings unchanged
    /// - Long strings truncated with ellipsis
    #[test]
    fn test_string_truncation() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("this is a long string", 10), "this is...");
        // String that fits exactly is not truncated
        assert_eq!(truncate_string("abc", 3), "abc");
        // String longer than max_len but max_len <= 3 returns just "..."
        assert_eq!(truncate_string("abcdef", 3), "...");
        assert_eq!(truncate_string("ab", 5), "ab");
    }

    /// # Status Symbols
    ///
    /// Verifies status symbols are correct.
    ///
    /// ## Test Scenario
    /// - Checks each status variant
    ///
    /// ## Expected Outcome
    /// - Each status has a unique symbol
    #[test]
    fn test_status_symbols() {
        assert_eq!(
            OutputWriter::<Vec<u8>>::status_symbol(&ItemStatus::Pending),
            "○"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::status_symbol(&ItemStatus::InProgress),
            "◐"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::status_symbol(&ItemStatus::Success),
            "✓"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::status_symbol(&ItemStatus::Failed),
            "✗"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::status_symbol(&ItemStatus::Skipped),
            "⊘"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::status_symbol(&ItemStatus::Conflict),
            "⚠"
        );
    }

    /// # Summary Text Formatting
    ///
    /// Verifies summary is formatted nicely for text output.
    ///
    /// ## Test Scenario
    /// - Creates SummaryInfo and writes with text formatter
    ///
    /// ## Expected Outcome
    /// - Contains result, counts, and version info
    #[test]
    fn test_summary_text_formatting() {
        use super::super::events::{SummaryCounts, SummaryInfo, SummaryResult};

        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        let summary = SummaryInfo {
            result: SummaryResult::Success,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            counts: SummaryCounts::new(3, 1, 1, 0),
            items: None,
            post_merge: None,
        };

        writer.write_summary(&summary).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("SUCCESS"));
        assert!(output.contains("v1.0.0"));
        assert!(output.contains("Successful: 3"));
        assert!(output.contains("Failed:     1"));
        assert!(output.contains("Skipped:    1"));
    }
}
