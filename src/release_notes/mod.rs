//! Release notes generation from version commits.
//!
//! This module extracts work item references from git commit messages
//! and generates formatted release notes by fetching work item titles
//! from Azure DevOps.
//!
//! # Features
//!
//! - Extract task IDs from `rwi:#XXXXX` patterns in commit messages
//! - Support version ranges (--from / --to)
//! - Group tasks by type (feat, fix, refactor)
//! - Multiple output formats (markdown, json, plain)
//! - Work item title caching

pub mod cache;

use crate::models::{
    CherryPickItem, CherryPickStatus, PullRequestWithWorkItems, ReleaseNotesOutputFormat,
    TaskGroup, WorkItem,
};
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

/// Represents a release note entry with task ID, title, and optional PR info.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReleaseNoteEntry {
    pub task_id: i32,
    pub title: String,
    pub url: String,
    pub group: TaskGroup,
    pub pr_id: Option<i32>,
    pub pr_url: Option<String>,
}

/// Represents a commit with extracted task information.
#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub hash: String,
    pub message: String,
    pub task_ids: Vec<i32>,
    pub group: TaskGroup,
}

/// Get the last commit message from a git repository.
///
/// Returns the full commit message body of the most recent commit.
pub fn get_last_commit_message(repo_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "-1", "--format=%B"])
        .output()
        .context("Failed to execute git log")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get commit message: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get commits in a range between two tags/refs.
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository
/// * `from` - Starting ref (tag, branch, or commit)
/// * `to` - Ending ref (defaults to HEAD if None)
///
/// # Returns
///
/// Vector of CommitEntry objects for commits in the range.
pub fn get_commits_in_range(
    repo_path: &Path,
    from: &str,
    to: Option<&str>,
) -> Result<Vec<CommitEntry>> {
    let to_ref = to.unwrap_or("HEAD");
    let range = format!("{}..{}", from, to_ref);

    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", &range, "--format=%H|%s|%b", "--no-merges"])
        .output()
        .context("Failed to execute git log for range")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get commits in range: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();

    for entry in output_str.split("\n\n") {
        if entry.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = entry.splitn(3, '|').collect();
        if parts.len() >= 2 {
            let hash = parts[0].trim().to_string();
            let subject = parts[1].trim().to_string();
            let body = parts.get(2).map(|s| s.trim()).unwrap_or("");

            let full_message = format!("{}\n{}", subject, body);
            let task_ids = extract_task_ids(&full_message);
            let group = determine_task_group(&subject);

            commits.push(CommitEntry {
                hash,
                message: full_message,
                task_ids,
                group,
            });
        }
    }

    Ok(commits)
}

/// Extract task IDs from commit message using "rwi:#XXXXX" pattern.
///
/// Returns a vector of unique task IDs found in the message, preserving order.
pub fn extract_task_ids(commit_message: &str) -> Vec<i32> {
    let re = Regex::new(r"rwi:#(\d+)").expect("Invalid regex pattern");

    let mut ids: Vec<i32> = re
        .captures_iter(commit_message)
        .filter_map(|cap| cap.get(1).and_then(|m| m.as_str().parse().ok()))
        .collect();

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(*id));

    ids
}

/// Determine task group based on commit message prefix.
///
/// Recognizes conventional commit prefixes:
/// - `feat:`, `feature:` -> Feature
/// - `fix:`, `bugfix:` -> Fix
/// - `refactor:` -> Refactor
/// - Everything else -> Other
pub fn determine_task_group(commit_message: &str) -> TaskGroup {
    let msg_lower = commit_message.to_lowercase();
    let trimmed = msg_lower.trim_start();

    if trimmed.starts_with("feat:") || trimmed.starts_with("feature:") {
        TaskGroup::Feature
    } else if trimmed.starts_with("fix:") || trimmed.starts_with("bugfix:") {
        TaskGroup::Fix
    } else if trimmed.starts_with("refactor:") {
        TaskGroup::Refactor
    } else {
        TaskGroup::Other
    }
}

/// Encode characters in a URL that break markdown link syntax.
///
/// Azure DevOps organization and project names may contain spaces
/// or other special characters. This function encodes them so that
/// `[text](url)` renders correctly in markdown previews.
fn encode_url_for_markdown(url: &str) -> String {
    url.replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
}

/// Generate release note entries from commits and work items.
///
/// # Arguments
///
/// * `commits` - Vector of commit entries with task IDs
/// * `work_items_map` - Map of work item ID to WorkItem
/// * `base_url` - Base URL for Azure DevOps (e.g., "https://org.visualstudio.com/project")
///
/// # Returns
///
/// Vector of ReleaseNoteEntry objects.
pub fn generate_entries(
    commits: &[CommitEntry],
    work_items_map: &HashMap<i32, WorkItem>,
    base_url: &str,
) -> Vec<ReleaseNoteEntry> {
    let mut entries = Vec::new();
    let mut seen_task_ids = std::collections::HashSet::new();

    for commit in commits {
        for &task_id in &commit.task_ids {
            // Skip duplicates
            if !seen_task_ids.insert(task_id) {
                continue;
            }

            let title = work_items_map
                .get(&task_id)
                .and_then(|wi| wi.fields.title.clone())
                .unwrap_or_else(|| "(Title not found)".to_string());

            let url = encode_url_for_markdown(&format!("{}/_workitems/edit/{}", base_url, task_id));

            entries.push(ReleaseNoteEntry {
                task_id,
                title,
                url,
                group: commit.group,
                pr_id: None,
                pr_url: None,
            });
        }
    }

    entries
}

/// Format entries as a markdown table.
pub fn format_markdown(entries: &[ReleaseNoteEntry], grouped: bool) -> String {
    if !grouped {
        return format_markdown_flat(entries);
    }

    let mut output = String::new();
    let mut groups: HashMap<TaskGroup, Vec<&ReleaseNoteEntry>> = HashMap::new();

    for entry in entries {
        groups.entry(entry.group).or_default().push(entry);
    }

    // Output groups in order: Features, Fixes, Refactors, Other
    for group in [
        TaskGroup::Feature,
        TaskGroup::Fix,
        TaskGroup::Refactor,
        TaskGroup::Other,
    ] {
        if let Some(group_entries) = groups.get(&group)
            && !group_entries.is_empty()
        {
            output.push_str(&format!("\n## {}\n\n", group));
            output.push_str("| Task ID | Title |\n");
            output.push_str("|---------|-------|\n");

            for entry in group_entries {
                output.push_str(&format!(
                    "| [#{}]({}) | {} |\n",
                    entry.task_id, entry.url, entry.title
                ));
            }
        }
    }

    output
}

/// Format entries as a flat markdown table (no grouping).
fn format_markdown_flat(entries: &[ReleaseNoteEntry]) -> String {
    let mut output = String::new();
    output.push_str("| Task ID | Title |\n");
    output.push_str("|---------|-------|\n");

    for entry in entries {
        output.push_str(&format!(
            "| [#{}]({}) | {} |\n",
            entry.task_id, entry.url, entry.title
        ));
    }

    output
}

/// Format entries as JSON.
pub fn format_json(entries: &[ReleaseNoteEntry], grouped: bool) -> Result<String> {
    if !grouped {
        return serde_json::to_string_pretty(entries).context("Failed to serialize to JSON");
    }

    // Group entries by task group
    let mut groups: HashMap<String, Vec<&ReleaseNoteEntry>> = HashMap::new();
    groups.insert("features".to_string(), Vec::new());
    groups.insert("fixes".to_string(), Vec::new());
    groups.insert("refactors".to_string(), Vec::new());
    groups.insert("other".to_string(), Vec::new());

    for entry in entries {
        let key = match entry.group {
            TaskGroup::Feature => "features",
            TaskGroup::Fix => "fixes",
            TaskGroup::Refactor => "refactors",
            TaskGroup::Other => "other",
        };
        groups.get_mut(key).unwrap().push(entry);
    }

    serde_json::to_string_pretty(&groups).context("Failed to serialize grouped JSON")
}

/// Format entries as plain text.
pub fn format_plain(entries: &[ReleaseNoteEntry], grouped: bool) -> String {
    if !grouped {
        return entries
            .iter()
            .map(|e| format!("#{}: {}", e.task_id, e.title))
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut output = String::new();
    let mut groups: HashMap<TaskGroup, Vec<&ReleaseNoteEntry>> = HashMap::new();

    for entry in entries {
        groups.entry(entry.group).or_default().push(entry);
    }

    for group in [
        TaskGroup::Feature,
        TaskGroup::Fix,
        TaskGroup::Refactor,
        TaskGroup::Other,
    ] {
        if let Some(group_entries) = groups.get(&group)
            && !group_entries.is_empty()
        {
            output.push_str(&format!("\n# {}\n", group));
            for entry in group_entries {
                output.push_str(&format!("#{}: {}\n", entry.task_id, entry.title));
            }
        }
    }

    output
}

/// Format entries based on output format.
pub fn format_output(
    entries: &[ReleaseNoteEntry],
    format: ReleaseNotesOutputFormat,
    grouped: bool,
) -> Result<String> {
    match format {
        ReleaseNotesOutputFormat::Markdown => Ok(format_markdown(entries, grouped)),
        ReleaseNotesOutputFormat::Json => format_json(entries, grouped),
        ReleaseNotesOutputFormat::Plain => Ok(format_plain(entries, grouped)),
    }
}

/// Copy text to system clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    use arboard::Clipboard;

    let mut clipboard = Clipboard::new().context("Failed to access clipboard")?;
    clipboard
        .set_text(text)
        .context("Failed to copy to clipboard")?;

    Ok(())
}

/// Resolve repository path from alias or path.
///
/// # Arguments
///
/// * `path_or_alias` - Optional path or alias (e.g., "th", "/path/to/repo")
/// * `aliases` - Map of alias names to paths from config
///
/// # Returns
///
/// Resolved PathBuf to the repository.
pub fn resolve_repo_path(
    path_or_alias: Option<&str>,
    aliases: &Option<HashMap<String, String>>,
) -> Result<std::path::PathBuf> {
    match path_or_alias {
        None => {
            // Use current directory
            std::env::current_dir().context("Failed to get current directory")
        }
        Some(input) => {
            // Check if it's an alias
            if let Some(alias_map) = aliases
                && let Some(path) = alias_map.get(input)
            {
                return Ok(std::path::PathBuf::from(path));
            }

            // Treat as path
            let path = std::path::PathBuf::from(input);
            if path.exists() {
                Ok(path)
            } else {
                anyhow::bail!(
                    "Path '{}' does not exist. If this is an alias, configure it in ~/.config/mergers/config.toml under [repo_aliases]",
                    input
                )
            }
        }
    }
}

/// Get all unique task IDs from a list of commits.
pub fn collect_task_ids(commits: &[CommitEntry]) -> Vec<i32> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for commit in commits {
        for &id in &commit.task_ids {
            if seen.insert(id) {
                ids.push(id);
            }
        }
    }

    ids
}

/// Generate release notes markdown from TUI merge data.
///
/// This function builds release notes directly from cherry-pick results
/// and their associated pull requests/work items, without requiring
/// git commit parsing.
///
/// # Arguments
///
/// * `version` - Version string (e.g., "v1.0.0")
/// * `cherry_pick_items` - All cherry-pick items with their statuses
/// * `pull_requests` - All PRs with associated work items
/// * `organization` - Azure DevOps organization name
/// * `project` - Azure DevOps project name
pub fn generate_from_merge_data(
    version: &str,
    cherry_pick_items: &[CherryPickItem],
    pull_requests: &[PullRequestWithWorkItems],
    organization: &str,
    project: &str,
) -> String {
    let base_url = format!("https://dev.azure.com/{}/{}", organization, project);
    let today = chrono::Local::now().format("%Y-%m-%d");

    let mut output = format!("# Release Notes - {version}\n\n**Release Date:** {today}\n");

    // Build work item entries from successful cherry-picks
    let successful_pr_ids: HashSet<i32> = cherry_pick_items
        .iter()
        .filter(|item| matches!(item.status, CherryPickStatus::Success))
        .map(|item| item.pr_id)
        .collect();

    let mut entries: Vec<ReleaseNoteEntry> = Vec::new();
    let mut seen_task_ids = HashSet::new();

    for pr_with_wi in pull_requests {
        if !successful_pr_ids.contains(&pr_with_wi.pr.id) {
            continue;
        }

        let group = determine_task_group(&pr_with_wi.pr.title);

        for wi in &pr_with_wi.work_items {
            if !seen_task_ids.insert(wi.id) {
                continue;
            }

            let title = wi
                .fields
                .title
                .clone()
                .unwrap_or_else(|| "(Title not found)".to_string());
            let url = encode_url_for_markdown(&format!("{}/_workitems/edit/{}", base_url, wi.id));

            entries.push(ReleaseNoteEntry {
                task_id: wi.id,
                title,
                url,
                group,
                pr_id: None,
                pr_url: None,
            });
        }
    }

    if entries.is_empty() {
        output.push_str("\nNo changes included in this release.\n");
        return output;
    }

    // Group entries by TaskGroup
    let mut groups: HashMap<TaskGroup, Vec<&ReleaseNoteEntry>> = HashMap::new();
    for entry in &entries {
        groups.entry(entry.group).or_default().push(entry);
    }

    // Output groups in order: Features, Fixes, Refactors, Other
    for group in [
        TaskGroup::Feature,
        TaskGroup::Fix,
        TaskGroup::Refactor,
        TaskGroup::Other,
    ] {
        if let Some(group_entries) = groups.get(&group)
            && !group_entries.is_empty()
        {
            output.push_str(&format!("\n## {}\n\n", group));
            for entry in group_entries {
                output.push_str(&format!(
                    "- [#{}]({}) {} \n",
                    entry.task_id, entry.url, entry.title
                ));
            }
        }
    }

    // Summary
    output.push_str(&format!(
        "\n---\n\n*{} work item(s) included in this release.*\n",
        entries.len()
    ));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_task_ids_single() {
        let message = "Version 1.0.0\n\nrwi:#12345";
        let ids = extract_task_ids(message);
        assert_eq!(ids, vec![12345]);
    }

    #[test]
    fn test_extract_task_ids_multiple() {
        let message = "Release notes\n\nrwi:#111\nrwi:#222\nrwi:#333";
        let ids = extract_task_ids(message);
        assert_eq!(ids, vec![111, 222, 333]);
    }

    #[test]
    fn test_extract_task_ids_duplicates() {
        let message = "rwi:#100\nrwi:#200\nrwi:#100";
        let ids = extract_task_ids(message);
        assert_eq!(ids, vec![100, 200]); // Duplicates removed
    }

    #[test]
    fn test_extract_task_ids_none() {
        let message = "No task references here";
        let ids = extract_task_ids(message);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_extract_task_ids_mixed_content() {
        let message = "feat: Add login feature\n\nrwi:#12345\nSome text here\nrwi:#67890";
        let ids = extract_task_ids(message);
        assert_eq!(ids, vec![12345, 67890]);
    }

    #[test]
    fn test_determine_task_group_feature() {
        assert_eq!(
            determine_task_group("feat: Add new feature"),
            TaskGroup::Feature
        );
        assert_eq!(
            determine_task_group("feature: New feature"),
            TaskGroup::Feature
        );
        assert_eq!(determine_task_group("FEAT: uppercase"), TaskGroup::Feature);
    }

    #[test]
    fn test_determine_task_group_fix() {
        assert_eq!(determine_task_group("fix: Fix bug"), TaskGroup::Fix);
        assert_eq!(determine_task_group("bugfix: Fix issue"), TaskGroup::Fix);
        assert_eq!(determine_task_group("FIX: uppercase"), TaskGroup::Fix);
    }

    #[test]
    fn test_determine_task_group_refactor() {
        assert_eq!(
            determine_task_group("refactor: Clean up code"),
            TaskGroup::Refactor
        );
        assert_eq!(
            determine_task_group("REFACTOR: uppercase"),
            TaskGroup::Refactor
        );
    }

    #[test]
    fn test_determine_task_group_other() {
        assert_eq!(determine_task_group("chore: Update deps"), TaskGroup::Other);
        assert_eq!(determine_task_group("docs: Add readme"), TaskGroup::Other);
        assert_eq!(
            determine_task_group("Random commit message"),
            TaskGroup::Other
        );
    }

    #[test]
    fn test_format_markdown_flat() {
        let entries = vec![ReleaseNoteEntry {
            task_id: 123,
            title: "Test task".to_string(),
            url: "https://example.com/123".to_string(),
            group: TaskGroup::Feature,
            pr_id: None,
            pr_url: None,
        }];

        let output = format_markdown(&entries, false);
        assert!(output.contains("| Task ID | Title |"));
        assert!(output.contains("[#123](https://example.com/123)"));
        assert!(output.contains("Test task"));
    }

    #[test]
    fn test_format_plain() {
        let entries = vec![ReleaseNoteEntry {
            task_id: 456,
            title: "Another task".to_string(),
            url: "https://example.com/456".to_string(),
            group: TaskGroup::Fix,
            pr_id: None,
            pr_url: None,
        }];

        let output = format_plain(&entries, false);
        assert_eq!(output, "#456: Another task");
    }

    #[test]
    fn test_resolve_repo_path_with_alias() {
        let mut aliases = HashMap::new();
        aliases.insert("test".to_string(), "/tmp/test-repo".to_string());

        let result = resolve_repo_path(Some("test"), &Some(aliases));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), std::path::PathBuf::from("/tmp/test-repo"));
    }

    #[test]
    fn test_resolve_repo_path_current_dir() {
        let result = resolve_repo_path(None, &None);
        assert!(result.is_ok());
    }
}
