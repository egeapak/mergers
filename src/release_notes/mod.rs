//! Release notes generation from Azure DevOps pull requests and work items.
//!
//! This module generates formatted release notes by fetching PR data
//! and associated work items from Azure DevOps, using PR labels (tags)
//! as the source of truth for version tracking.
//!
//! # Features
//!
//! - PR label/tag-based version tracking
//! - Group tasks by type (feat, fix, refactor)
//! - Multiple output formats (markdown, json, plain)
//! - Work item caching

pub mod cache;

use crate::models::{
    CherryPickItem, CherryPickStatus, PullRequestWithWorkItems, ReleaseNotesOutputFormat, TaskGroup,
};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

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
    use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

    const MARKDOWN_ENCODE_SET: &AsciiSet =
        &CONTROLS.add(b' ').add(b'(').add(b')').add(b'[').add(b']');

    utf8_percent_encode(url, MARKDOWN_ENCODE_SET).to_string()
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
                    "| [{}]({}) | {} |\n",
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
            "| [{}]({}) | {} |\n",
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
                    "- [{}]({}) {} \n",
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

/// Build release note entries from PR + work item data.
pub fn build_entries_from_prs(
    prs: &[PullRequestWithWorkItems],
    organization: &str,
    project: &str,
) -> Vec<ReleaseNoteEntry> {
    let base_url = format!("https://dev.azure.com/{}/{}", organization, project);
    let mut entries: Vec<ReleaseNoteEntry> = Vec::new();
    let mut seen_task_ids = HashSet::new();

    for pr_with_wi in prs {
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
                pr_id: Some(pr_with_wi.pr.id),
                pr_url: Some(encode_url_for_markdown(&format!(
                    "{}/_git/pullrequest/{}",
                    base_url, pr_with_wi.pr.id
                ))),
            });
        }
    }

    entries
}

/// Generate full release notes markdown from PR + work item data.
pub fn generate_from_prs(
    version: &str,
    prs: &[PullRequestWithWorkItems],
    organization: &str,
    project: &str,
) -> String {
    let entries = build_entries_from_prs(prs, organization, project);
    let today = chrono::Local::now().format("%Y-%m-%d");
    let mut output = format!("# Release Notes - {version}\n\n**Release Date:** {today}\n");

    if entries.is_empty() {
        output.push_str("\nNo changes included in this release.\n");
        return output;
    }

    let mut groups: HashMap<TaskGroup, Vec<&ReleaseNoteEntry>> = HashMap::new();
    for entry in &entries {
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
            output.push_str(&format!("\n## {}\n\n", group));
            for entry in group_entries {
                output.push_str(&format!(
                    "- [{}]({}) {} \n",
                    entry.task_id, entry.url, entry.title
                ));
            }
        }
    }

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
        assert!(output.contains("[123](https://example.com/123)"));
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
}
