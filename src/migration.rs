use anyhow::Result;
use std::path::Path;

use crate::{
    api::AzureDevOpsClient,
    git::{check_commit_exists_in_branch, check_pr_merged_in_branch},
    models::{MigrationAnalysis, PRAnalysisResult, PullRequestWithWorkItems, SymmetricDiffResult},
};

#[derive(Clone)]
pub struct MigrationAnalyzer {
    client: AzureDevOpsClient,
    terminal_states: Vec<String>,
}

impl MigrationAnalyzer {
    pub fn new(client: AzureDevOpsClient, terminal_states: Vec<String>) -> Self {
        Self {
            client,
            terminal_states,
        }
    }

    pub async fn analyze_single_pr(
        &self,
        pr_with_work_items: &PullRequestWithWorkItems,
        _symmetric_diff: &SymmetricDiffResult,
        repo_path: &Path,
        target_branch: &str,
    ) -> Result<PRAnalysisResult> {
        // Get commit ID from PR
        let commit_id = if let Some(last_merge_commit) = &pr_with_work_items.pr.last_merge_commit {
            last_merge_commit.commit_id.clone()
        } else {
            // If no lastMergeCommit, we can't analyze this PR
            return Ok(PRAnalysisResult {
                pr: pr_with_work_items.clone(),
                all_work_items_terminal: false,
                terminal_work_items: Vec::new(),
                non_terminal_work_items: pr_with_work_items.work_items.clone(),
                commit_in_target: false,
                commit_title_in_target: false,
                commit_id: String::new(),
                unsure_reason: Some("No lastMergeCommit available".to_string()),
            });
        };

        // Check if commit exists in target branch
        let commit_in_target =
            check_commit_exists_in_branch(repo_path, &commit_id, target_branch).unwrap_or(false);

        // Check if PR was merged using Azure DevOps merge pattern
        let commit_title_in_target = check_pr_merged_in_branch(
            repo_path,
            pr_with_work_items.pr.id,
            &pr_with_work_items.pr.title,
            target_branch,
        )
        .unwrap_or(false);

        // Analyze work items
        let (all_work_items_terminal, terminal_work_items, non_terminal_work_items) = self
            .client
            .analyze_work_items_for_pr(pr_with_work_items, &self.terminal_states);

        // Determine if PR is actually merged (either commit ID or title found)
        let actually_merged = commit_in_target || commit_title_in_target;

        // Handle PRs with no work items - skip work item check
        let has_work_items = !pr_with_work_items.work_items.is_empty();
        let work_items_requirement_met = if has_work_items {
            all_work_items_terminal
        } else {
            true // Skip work item check if no work items
        };

        // Generate unsure reason if applicable
        let unsure_reason = match (work_items_requirement_met, actually_merged, has_work_items) {
            (true, true, _) => None,   // Eligible
            (false, false, _) => None, // Not merged
            (true, false, true) => Some(
                "Work items are in terminal state but PR not found in target branch".to_string(),
            ),
            (true, false, false) => {
                Some("No work items found and PR not found in target branch".to_string())
            }
            (false, true, true) => {
                // PR is in target branch but work items are not in terminal state
                let non_terminal_details: Vec<String> = non_terminal_work_items
                    .iter()
                    .map(|wi| {
                        format!(
                            "#{} ({})",
                            wi.id,
                            wi.fields.state.as_deref().unwrap_or("Unknown")
                        )
                    })
                    .collect();
                Some(format!(
                    "PR is in target branch but work items are not in terminal state: {}",
                    non_terminal_details.join(", ")
                ))
            }
            (false, true, false) => unreachable!(), // Can't have false work_items_requirement_met with no work items
        };

        Ok(PRAnalysisResult {
            pr: pr_with_work_items.clone(),
            all_work_items_terminal: work_items_requirement_met,
            terminal_work_items,
            non_terminal_work_items,
            commit_in_target,
            commit_title_in_target,
            commit_id,
            unsure_reason,
        })
    }

    pub fn categorize_prs(
        &self,
        analyses: Vec<PRAnalysisResult>,
        symmetric_diff: SymmetricDiffResult,
    ) -> Result<MigrationAnalysis> {
        let mut eligible = Vec::new();
        let mut unsure = Vec::new();
        let mut not_merged = Vec::new();
        let mut unsure_details = Vec::new();

        for analysis in analyses {
            // Use enhanced logic: PR is actually merged if commit ID OR title is found
            let actually_merged = analysis.commit_in_target || analysis.commit_title_in_target;

            match (analysis.all_work_items_terminal, actually_merged) {
                (true, true) => {
                    // PR is in both lists - work items requirement met AND PR is actually merged
                    eligible.push(analysis.pr.clone());
                }
                (true, false) | (false, true) => {
                    // One condition met but not the other - needs manual review
                    unsure.push(analysis.pr.clone());
                    unsure_details.push(analysis);
                }
                (false, false) => {
                    // Neither condition met - not merged
                    not_merged.push(analysis.pr.clone());
                }
            }
        }

        Ok(MigrationAnalysis {
            eligible_prs: eligible,
            unsure_prs: unsure,
            not_merged_prs: not_merged,
            terminal_states: self.terminal_states.clone(),
            symmetric_diff,
            unsure_details,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreatedBy, MergeCommit, PullRequest, WorkItem, WorkItemFields};

    fn create_test_pr(id: i32, title: &str, commit_id: Option<String>) -> PullRequest {
        PullRequest {
            id,
            title: title.to_string(),
            closed_date: Some("2023-01-01T00:00:00Z".to_string()),
            created_by: CreatedBy {
                display_name: "Test User".to_string(),
            },
            last_merge_commit: commit_id.map(|id| MergeCommit { commit_id: id }),
            labels: None,
        }
    }

    fn create_test_work_item(id: i32, state: &str) -> WorkItem {
        WorkItem {
            id,
            fields: WorkItemFields {
                title: Some("Test Work Item".to_string()),
                state: Some(state.to_string()),
                work_item_type: Some("Bug".to_string()),
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
                created_date: None,
            },
            history: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_categorize_prs() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec![
            "Closed".to_string(),
            "Next Closed".to_string(),
            "Next Merged".to_string(),
        ];
        let analyzer = MigrationAnalyzer::new(client, terminal_states);

        let symmetric_diff = SymmetricDiffResult {
            commits_in_dev_not_target: Vec::new(),
            commits_in_target_not_dev: Vec::new(),
            common_commits: Vec::new(),
        };

        // Test eligible PR (terminal work items + commit in target)
        let eligible_pr = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "Eligible PR", Some("abc123".to_string())),
                work_items: vec![create_test_work_item(1, "Closed")],
                selected: false,
            },
            all_work_items_terminal: true,
            terminal_work_items: vec![create_test_work_item(1, "Closed")],
            non_terminal_work_items: Vec::new(),
            commit_in_target: true,
            commit_title_in_target: false,
            commit_id: "abc123".to_string(),
            unsure_reason: None,
        };

        let analyses = vec![eligible_pr];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 0);
        assert_eq!(result.not_merged_prs.len(), 0);
    }

    #[tokio::test]
    async fn test_enhanced_categorization() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec![
            "Closed".to_string(),
            "Next Closed".to_string(),
            "Next Merged".to_string(),
        ];
        let analyzer = MigrationAnalyzer::new(client, terminal_states);

        let symmetric_diff = SymmetricDiffResult {
            commits_in_dev_not_target: Vec::new(),
            commits_in_target_not_dev: Vec::new(),
            common_commits: Vec::new(),
        };

        // Test case 1: PR with title match but no commit ID match (should be eligible)
        let title_match_pr = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "Fixed bug in auth", Some("abc123".to_string())),
                work_items: vec![create_test_work_item(1, "Closed")],
                selected: false,
            },
            all_work_items_terminal: true,
            terminal_work_items: vec![create_test_work_item(1, "Closed")],
            non_terminal_work_items: Vec::new(),
            commit_in_target: false,
            commit_title_in_target: true,
            commit_id: "abc123".to_string(),
            unsure_reason: None,
        };

        // Test case 2: PR with terminal work items but not in target (should be unsure)
        let unsure_pr = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(2, "Another PR", Some("def456".to_string())),
                work_items: vec![create_test_work_item(2, "Closed")],
                selected: false,
            },
            all_work_items_terminal: true,
            terminal_work_items: vec![create_test_work_item(2, "Closed")],
            non_terminal_work_items: Vec::new(),
            commit_in_target: false,
            commit_title_in_target: false,
            commit_id: "def456".to_string(),
            unsure_reason: Some(
                "Work items are in terminal state but PR not found in target branch".to_string(),
            ),
        };

        let analyses = vec![title_match_pr, unsure_pr];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 1);
        assert_eq!(result.not_merged_prs.len(), 0);
        assert_eq!(result.unsure_details.len(), 1);
        assert!(result.unsure_details[0].unsure_reason.is_some());
    }

    #[tokio::test]
    async fn test_no_work_items_handling() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec![
            "Closed".to_string(),
            "Next Closed".to_string(),
            "Next Merged".to_string(),
        ];
        let analyzer = MigrationAnalyzer::new(client, terminal_states);

        let symmetric_diff = SymmetricDiffResult {
            commits_in_dev_not_target: Vec::new(),
            commits_in_target_not_dev: Vec::new(),
            common_commits: Vec::new(),
        };

        // Test PR with no work items but in target branch (should be eligible)
        let no_work_items_pr = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "PR with no work items", Some("abc123".to_string())),
                work_items: Vec::new(), // No work items
                selected: false,
            },
            all_work_items_terminal: true, // Should be true because no work items = skip check
            terminal_work_items: Vec::new(),
            non_terminal_work_items: Vec::new(),
            commit_in_target: true,
            commit_title_in_target: false,
            commit_id: "abc123".to_string(),
            unsure_reason: None,
        };

        let analyses = vec![no_work_items_pr];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 0);
        assert_eq!(result.not_merged_prs.len(), 0);
    }

    #[tokio::test]
    async fn test_work_item_details_in_unsure_reason() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec![
            "Closed".to_string(),
            "Next Closed".to_string(),
            "Next Merged".to_string(),
        ];
        let analyzer = MigrationAnalyzer::new(client, terminal_states);

        let symmetric_diff = SymmetricDiffResult {
            commits_in_dev_not_target: Vec::new(),
            commits_in_target_not_dev: Vec::new(),
            common_commits: Vec::new(),
        };

        // Test PR with non-terminal work items but in target branch (should be unsure with details)
        let non_terminal_work_items = vec![
            create_test_work_item(1, "Active"),
            create_test_work_item(2, "In Progress"),
        ];

        let pr_with_non_terminal_work_items = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "PR with non-terminal work items", Some("abc123".to_string())),
                work_items: non_terminal_work_items.clone(),
                selected: false,
            },
            all_work_items_terminal: false,
            terminal_work_items: Vec::new(),
            non_terminal_work_items,
            commit_in_target: true,
            commit_title_in_target: false,
            commit_id: "abc123".to_string(),
            unsure_reason: Some("PR is in target branch but work items are not in terminal state: #1 (Active), #2 (In Progress)".to_string()),
        };

        let analyses = vec![pr_with_non_terminal_work_items];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 0);
        assert_eq!(result.unsure_prs.len(), 1);
        assert_eq!(result.not_merged_prs.len(), 0);
        assert_eq!(result.unsure_details.len(), 1);

        let unsure_reason = result.unsure_details[0].unsure_reason.as_ref().unwrap();
        assert!(unsure_reason.contains("#1 (Active)"));
        assert!(unsure_reason.contains("#2 (In Progress)"));
    }
}
