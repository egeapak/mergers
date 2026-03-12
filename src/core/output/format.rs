//! Output formatters for different output modes.
//!
//! This module provides formatters for text, JSON, and NDJSON output modes,
//! each implementing the `OutputFormatter` trait for consistent behavior.

use super::events::{
    ConflictInfo, ItemStatus, PostMergeStatus, ProgressEvent, StatusInfo, SummaryInfo,
    SummaryResult,
};
use crate::models::OutputFormat;
use crate::utils::truncate_str;
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
                ..
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
            ProgressEvent::DependencyAnalysisStart { pr_count } => {
                self.writeln(&format!("Analyzing dependencies for {} PRs...", pr_count))?;
            }
            ProgressEvent::DependencyAnalysisComplete {
                independent,
                partial,
                dependent,
            } => {
                self.writeln(&format!(
                    "  Dependencies: {} independent, {} partial, {} overlapping",
                    independent, partial, dependent
                ))?;
            }
            ProgressEvent::DependencyWarning {
                selected_pr_id,
                unselected_pr_id,
                unselected_pr_title,
                is_critical,
                shared_files,
                ..
            } => {
                let severity = if *is_critical {
                    "⚠ CRITICAL"
                } else {
                    "⚡ Warning"
                };
                self.writeln(&format!(
                    "  {} PR #{} depends on unselected PR #{} ({})",
                    severity,
                    selected_pr_id,
                    unselected_pr_id,
                    truncate_string(unselected_pr_title, 30)
                ))?;
                if !shared_files.is_empty() {
                    self.writeln(&format!("    Shared files: {}", shared_files.join(", ")))?;
                }
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
            ProgressEvent::HookStart {
                trigger,
                command_count,
            } => {
                self.writeln(&format!(
                    "Running {} hook ({} command{})...",
                    trigger,
                    command_count,
                    if *command_count == 1 { "" } else { "s" }
                ))?;
            }
            ProgressEvent::HookCommandStart {
                trigger: _,
                command,
                index: _,
            } => {
                self.writeln(&format!("  → {}", command))?;
            }
            ProgressEvent::HookCommandComplete {
                trigger: _,
                command: _,
                success,
                index: _,
            } => {
                if *success {
                    self.writeln("    ✓ completed")?;
                } else {
                    self.writeln("    ✗ failed")?;
                }
            }
            ProgressEvent::HookComplete {
                trigger,
                all_succeeded,
            } => {
                if *all_succeeded {
                    self.writeln(&format!("  ✓ {} hooks completed", trigger))?;
                } else {
                    self.writeln(&format!("  ✗ {} hooks failed", trigger))?;
                }
            }
            ProgressEvent::HookFailed {
                trigger,
                command,
                error,
            } => {
                self.writeln(&format!("Hook {} failed: {}", trigger, command))?;
                if !error.is_empty() {
                    self.writeln(&format!("  Error: {}", error))?;
                }
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
        format!("{}...", truncate_str(s, max_len - 3))
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
            state_file_path: None,
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
                state_file_path: None,
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
                state_file_path: None,
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

    /// # Post-Merge Status Symbols
    ///
    /// Verifies post-merge status symbols are correct.
    ///
    /// ## Test Scenario
    /// - Checks each post-merge status variant
    ///
    /// ## Expected Outcome
    /// - Each status has a unique symbol
    #[test]
    fn test_post_merge_status_symbols() {
        assert_eq!(
            OutputWriter::<Vec<u8>>::post_merge_status_symbol(&PostMergeStatus::Pending),
            "○"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::post_merge_status_symbol(&PostMergeStatus::Success),
            "✓"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::post_merge_status_symbol(&PostMergeStatus::Failed {
                error: "test".to_string()
            }),
            "✗"
        );
        assert_eq!(
            OutputWriter::<Vec<u8>>::post_merge_status_symbol(&PostMergeStatus::Skipped),
            "⊘"
        );
    }

    /// # Cherry Pick Events Text Formatting
    ///
    /// Verifies cherry-pick events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes various cherry-pick events
    ///
    /// ## Expected Outcome
    /// - Output contains expected text for each event type
    #[test]
    fn test_cherry_pick_events_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        // CherryPickSuccess
        writer
            .write_event(&ProgressEvent::CherryPickSuccess {
                pr_id: 123,
                commit_id: "abc123".to_string(),
            })
            .unwrap();

        // CherryPickFailed
        writer
            .write_event(&ProgressEvent::CherryPickFailed {
                pr_id: 456,
                error: "merge conflict".to_string(),
            })
            .unwrap();

        // CherryPickSkipped
        writer
            .write_event(&ProgressEvent::CherryPickSkipped {
                pr_id: 789,
                reason: Some("already applied".to_string()),
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("PR #123 applied"));
        assert!(output.contains("PR #456 failed"));
        assert!(output.contains("merge conflict"));
        assert!(output.contains("PR #789 skipped"));
        assert!(output.contains("already applied"));
    }

    /// # Dependency Events Text Formatting
    ///
    /// Verifies dependency analysis events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes dependency analysis events
    ///
    /// ## Expected Outcome
    /// - Output contains expected text
    #[test]
    fn test_dependency_events_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::DependencyAnalysisStart { pr_count: 5 })
            .unwrap();

        writer
            .write_event(&ProgressEvent::DependencyAnalysisComplete {
                independent: 3,
                partial: 1,
                dependent: 1,
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Analyzing dependencies for 5 PRs"));
        assert!(output.contains("3 independent"));
        assert!(output.contains("1 partial"));
        assert!(output.contains("1 overlapping"));
    }

    /// # Hook Events Text Formatting
    ///
    /// Verifies hook events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes various hook events
    ///
    /// ## Expected Outcome
    /// - Output contains expected text for each event type
    #[test]
    fn test_hook_events_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::HookStart {
                trigger: "post_merge".to_string(),
                command_count: 2,
            })
            .unwrap();

        writer
            .write_event(&ProgressEvent::HookCommandStart {
                trigger: "post_merge".to_string(),
                command: "cargo test".to_string(),
                index: 0,
            })
            .unwrap();

        writer
            .write_event(&ProgressEvent::HookCommandComplete {
                trigger: "post_merge".to_string(),
                command: "cargo test".to_string(),
                success: true,
                index: 0,
            })
            .unwrap();

        writer
            .write_event(&ProgressEvent::HookComplete {
                trigger: "post_merge".to_string(),
                all_succeeded: true,
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Running post_merge hook"));
        assert!(output.contains("2 commands"));
        assert!(output.contains("cargo test"));
        assert!(output.contains("completed"));
    }

    /// # Summary Result Types Text Formatting
    ///
    /// Verifies different summary result types format correctly.
    ///
    /// ## Test Scenario
    /// - Writes summaries with different result types
    ///
    /// ## Expected Outcome
    /// - Each result type has correct heading
    #[test]
    fn test_summary_result_types_text_formatting() {
        use super::super::events::{SummaryCounts, SummaryInfo, SummaryResult};

        let test_cases = [
            (SummaryResult::Success, "SUCCESS"),
            (SummaryResult::PartialSuccess, "PARTIAL SUCCESS"),
            (SummaryResult::Failed, "FAILED"),
            (SummaryResult::Aborted, "ABORTED"),
            (SummaryResult::Conflict, "CONFLICT"),
        ];

        for (result, expected_text) in test_cases {
            let mut buffer = Vec::new();
            let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

            let summary = SummaryInfo {
                result: result.clone(),
                version: "v1.0.0".to_string(),
                target_branch: "main".to_string(),
                counts: SummaryCounts::new(1, 0, 0, 0),
                items: None,
                post_merge: None,
            };

            writer.write_summary(&summary).unwrap();

            let output = String::from_utf8(buffer).unwrap();
            assert!(
                output.contains(expected_text),
                "Expected '{}' in output for {:?}",
                expected_text,
                result
            );
        }
    }

    /// # Summary With Post-Merge Tasks
    ///
    /// Verifies summary includes post-merge task counts.
    ///
    /// ## Test Scenario
    /// - Writes summary with post_merge field set
    ///
    /// ## Expected Outcome
    /// - Output contains post-merge task counts
    #[test]
    fn test_summary_with_post_merge_tasks() {
        use super::super::events::{PostMergeSummary, SummaryCounts, SummaryInfo, SummaryResult};

        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        let summary = SummaryInfo {
            result: SummaryResult::Success,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            counts: SummaryCounts::new(3, 0, 0, 0),
            items: None,
            post_merge: Some(PostMergeSummary {
                total_tasks: 3,
                successful: 2,
                failed: 1,
                tasks: None,
            }),
        };

        writer.write_summary(&summary).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Post-merge tasks:"));
        assert!(output.contains("Successful: 2"));
        assert!(output.contains("Failed:     1"));
    }

    /// # NDJSON Conflict Info
    ///
    /// Verifies conflict info formats as NDJSON.
    ///
    /// ## Test Scenario
    /// - Writes conflict info with NDJSON formatter
    ///
    /// ## Expected Outcome
    /// - Output is valid JSON
    #[test]
    fn test_ndjson_conflict_info() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Ndjson, false);

        let conflict = ConflictInfo::new(
            123,
            "Test PR".to_string(),
            "abc123".to_string(),
            vec!["file1.rs".to_string()],
            PathBuf::from("/tmp/repo"),
        );

        writer.write_conflict(&conflict).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["pr_id"], 123);
    }

    /// # NDJSON Summary
    ///
    /// Verifies summary formats as NDJSON.
    ///
    /// ## Test Scenario
    /// - Writes summary with NDJSON formatter
    ///
    /// ## Expected Outcome
    /// - Output is valid JSON on single line
    #[test]
    fn test_ndjson_summary() {
        use super::super::events::{SummaryCounts, SummaryInfo, SummaryResult};

        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Ndjson, false);

        let summary = SummaryInfo {
            result: SummaryResult::Success,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            counts: SummaryCounts::new(3, 0, 0, 0),
            items: None,
            post_merge: None,
        };

        writer.write_summary(&summary).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["result"], "success");
        assert_eq!(parsed["version"], "v1.0.0");
    }

    /// # Complete Event Text Formatting
    ///
    /// Verifies complete event formats correctly.
    ///
    /// ## Test Scenario
    /// - Writes complete event
    ///
    /// ## Expected Outcome
    /// - Output contains counts
    #[test]
    fn test_complete_event_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::Complete {
                successful: 5,
                failed: 2,
                skipped: 1,
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("5 successful"));
        assert!(output.contains("2 failed"));
        assert!(output.contains("1 skipped"));
    }

    /// # Error Event Text Formatting
    ///
    /// Verifies error events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes error events with and without code
    ///
    /// ## Expected Outcome
    /// - Output contains error message and code when present
    #[test]
    fn test_error_event_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::Error {
                message: "Something went wrong".to_string(),
                code: Some("E001".to_string()),
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("[E001]"));
        assert!(output.contains("Something went wrong"));
    }

    /// # Aborted Event Text Formatting
    ///
    /// Verifies aborted events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes aborted events (success and failure)
    ///
    /// ## Expected Outcome
    /// - Output contains appropriate message
    #[test]
    fn test_aborted_event_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::Aborted {
                success: true,
                message: Some("User requested abort".to_string()),
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("aborted successfully"));
        assert!(output.contains("User requested abort"));
    }

    /// # Hook Failed Event Text Formatting
    ///
    /// Verifies hook failed events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes hook failed event
    ///
    /// ## Expected Outcome
    /// - Output contains trigger, command, and error
    #[test]
    fn test_hook_failed_event_text_formatting() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::HookFailed {
                trigger: "post_merge".to_string(),
                command: "cargo test".to_string(),
                error: "tests failed".to_string(),
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Hook post_merge failed"));
        assert!(output.contains("cargo test"));
        assert!(output.contains("tests failed"));
    }

    /// # Flush Operation
    ///
    /// Verifies flush doesn't error.
    ///
    /// ## Test Scenario
    /// - Calls flush on writer
    ///
    /// ## Expected Outcome
    /// - No error returned
    #[test]
    fn test_flush_operation() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);
        assert!(writer.flush().is_ok());
    }

    /// # Format and Quiet Getters
    ///
    /// Verifies format() and is_quiet() getters work.
    ///
    /// ## Test Scenario
    /// - Creates writers with different settings
    ///
    /// ## Expected Outcome
    /// - Getters return correct values
    #[test]
    fn test_format_and_quiet_getters() {
        let mut buffer = Vec::new();
        let writer = OutputWriter::new(&mut buffer, OutputFormat::Json, true);

        assert!(matches!(writer.format(), OutputFormat::Json));
        assert!(writer.is_quiet());
    }

    /// # Post-Merge Progress Event
    ///
    /// Verifies post-merge progress events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes post-merge progress events with different statuses
    ///
    /// ## Expected Outcome
    /// - Output contains task type, status, and target ID
    #[test]
    fn test_post_merge_progress_event() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::PostMergeStart { task_count: 3 })
            .unwrap();

        writer
            .write_event(&ProgressEvent::PostMergeProgress {
                task_type: "tag_pr".to_string(),
                target_id: 123,
                status: PostMergeStatus::Success,
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("3 post-merge tasks"));
        assert!(output.contains("tag_pr"));
        assert!(output.contains("123"));
    }

    /// # Dependency Warning Event
    ///
    /// Verifies dependency warning events format correctly.
    ///
    /// ## Test Scenario
    /// - Writes dependency warning event
    ///
    /// ## Expected Outcome
    /// - Output contains warning details
    #[test]
    fn test_dependency_warning_event() {
        let mut buffer = Vec::new();
        let mut writer = OutputWriter::new(&mut buffer, OutputFormat::Text, false);

        writer
            .write_event(&ProgressEvent::DependencyWarning {
                selected_pr_id: 100,
                selected_pr_title: "Feature PR".to_string(),
                unselected_pr_id: 200,
                unselected_pr_title: "Important dependency PR".to_string(),
                is_critical: true,
                shared_files: vec!["src/lib.rs".to_string()],
            })
            .unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("CRITICAL"));
        assert!(output.contains("PR #100"));
        assert!(output.contains("PR #200"));
        assert!(output.contains("src/lib.rs"));
    }
}
