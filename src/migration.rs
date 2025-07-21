use anyhow::Result;

use crate::{
    api::AzureDevOpsClient,
    git::{CommitHistory, check_commit_in_history, check_pr_merged_in_history},
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
        commit_history: &CommitHistory,
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
                reason: Some("No lastMergeCommit available".to_string()),
            });
        };

        // Check if commit exists in target branch using pre-fetched history
        let commit_in_target = check_commit_in_history(&commit_id, commit_history);

        // Check if PR was merged using comprehensive PR detection with pre-fetched history
        let commit_title_in_target = check_pr_merged_in_history(
            pr_with_work_items.pr.id,
            &pr_with_work_items.pr.title,
            commit_history,
        );

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

        // Generate detailed reasons for all cases with PR detection details
        let detection_details = Self::generate_pr_detection_details(
            commit_in_target,
            commit_title_in_target,
            &commit_id,
        );

        let (unsure_reason, reason) = match (
            work_items_requirement_met,
            actually_merged,
            has_work_items,
        ) {
            (true, true, _) => (
                None,
                Some(format!(
                    "Eligible: Work items in terminal state and PR found in target branch. {}",
                    detection_details
                )),
            ),
            (false, true, true) => {
                // PR is in target branch but work items are not in terminal state - now eligible
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
                (
                    None,
                    Some(format!(
                        "Eligible: PR found in target branch (work items not in terminal state but overridden): {}. {}",
                        non_terminal_details.join(", "),
                        detection_details
                    )),
                )
            }
            (false, true, false) => (
                None,
                Some(format!(
                    "Eligible: PR found in target branch and no work items to check. {}",
                    detection_details
                )),
            ),
            (true, false, true) => {
                let reason = "Work items are in terminal state but PR not found in target branch"
                    .to_string();
                (Some(reason.clone()), Some(format!("Unsure: {}", reason)))
            }
            (true, false, false) => {
                let reason = "No work items found and PR not found in target branch".to_string();
                (Some(reason.clone()), Some(format!("Unsure: {}", reason)))
            }
            (false, false, true) => {
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
                (
                    None,
                    Some(format!(
                        "Not merged: Work items not in terminal state and PR not found in target branch: {}. Detection attempts: commit ID '{}' not found in target, PR title/ID not found in commit history",
                        non_terminal_details.join(", "),
                        commit_id
                    )),
                )
            }
            (false, false, false) => (
                None,
                Some(format!(
                    "Not merged: No work items found and PR not found in target branch. Detection attempts: commit ID '{}' not found in target, PR title/ID not found in commit history",
                    commit_id
                )),
            ),
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
            reason,
        })
    }

    fn generate_pr_detection_details(
        commit_in_target: bool,
        commit_title_in_target: bool,
        commit_id: &str,
    ) -> String {
        match (commit_in_target, commit_title_in_target) {
            (true, true) => format!(
                "Detection: Commit '{}' found in target AND PR pattern found in commit history",
                commit_id
            ),
            (true, false) => format!("Detection: Commit '{}' found in target branch", commit_id),
            (false, true) => {
                "Detection: PR pattern found in commit history (commit ID not directly found)"
                    .to_string()
            }
            (false, false) => format!(
                "Detection: Commit '{}' not found in target, PR pattern not found in commit history",
                commit_id
            ),
        }
    }

    pub fn categorize_prs(
        &self,
        analyses: Vec<PRAnalysisResult>,
        symmetric_diff: SymmetricDiffResult,
    ) -> Result<MigrationAnalysis> {
        self.categorize_prs_with_overrides(analyses, symmetric_diff, Default::default())
    }

    pub fn categorize_prs_with_overrides(
        &self,
        analyses: Vec<PRAnalysisResult>,
        symmetric_diff: SymmetricDiffResult,
        manual_overrides: crate::models::ManualOverrides,
    ) -> Result<MigrationAnalysis> {
        let mut eligible = Vec::new();
        let mut unsure = Vec::new();
        let mut not_merged = Vec::new();
        let mut unsure_details = Vec::new();

        for analysis in &analyses {
            let pr_id = analysis.pr.pr.id;
            
            // Check for manual overrides first
            if manual_overrides.marked_as_not_eligible.contains(&pr_id) {
                // Manually marked as not eligible - always goes to not_merged regardless of automatic analysis
                not_merged.push(analysis.pr.clone());
                continue;
            }
            
            if manual_overrides.marked_as_eligible.contains(&pr_id) {
                // Manually marked as eligible - always goes to eligible regardless of automatic analysis
                eligible.push(analysis.pr.clone());
                continue;
            }
            
            // Use enhanced logic for automatic categorization: PR is actually merged if commit ID OR title is found
            let actually_merged = analysis.commit_in_target || analysis.commit_title_in_target;

            match (analysis.all_work_items_terminal, actually_merged) {
                (true, true) => {
                    // PR is in both lists - work items requirement met AND PR is actually merged
                    eligible.push(analysis.pr.clone());
                }
                (false, true) => {
                    // PR is actually merged (commit in target branch history) - mark as eligible regardless of work item state
                    eligible.push(analysis.pr.clone());
                }
                (true, false) => {
                    // Work items requirement met but PR not found in target - needs manual review
                    unsure.push(analysis.pr.clone());
                    unsure_details.push(analysis.clone());
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
            unsure_details: unsure_details.clone(),
            all_details: analyses,
            manual_overrides,
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
            reason: Some(
                "Eligible: Work items in terminal state and PR found in target branch".to_string(),
            ),
        };

        let analyses = vec![eligible_pr];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 0);
        assert_eq!(result.not_merged_prs.len(), 0);
        assert_eq!(result.all_details.len(), 1);
        assert!(result.manual_overrides.marked_as_eligible.is_empty());
        assert!(result.manual_overrides.marked_as_not_eligible.is_empty());
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
            reason: Some(
                "Eligible: Work items in terminal state and PR found in target branch".to_string(),
            ),
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
            reason: Some(
                "Unsure: Work items are in terminal state but PR not found in target branch"
                    .to_string(),
            ),
        };

        // Test case 3: PR with non-terminal work items but commit in target (should be eligible)
        let non_terminal_but_merged_pr = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(3, "Non-terminal but merged", Some("ghi789".to_string())),
                work_items: vec![create_test_work_item(3, "Active")],
                selected: false,
            },
            all_work_items_terminal: false,
            terminal_work_items: Vec::new(),
            non_terminal_work_items: vec![create_test_work_item(3, "Active")],
            commit_in_target: true,
            commit_title_in_target: false,
            commit_id: "ghi789".to_string(),
            unsure_reason: None,
            reason: Some("Eligible: PR found in target branch (work items not in terminal state but overridden): #3 (Active)".to_string()),
        };

        let analyses = vec![title_match_pr, unsure_pr, non_terminal_but_merged_pr];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 2);
        assert_eq!(result.unsure_prs.len(), 1);
        assert_eq!(result.not_merged_prs.len(), 0);
        assert_eq!(result.unsure_details.len(), 1);
        assert_eq!(result.all_details.len(), 3);
        assert!(result.unsure_details[0].unsure_reason.is_some());
        assert!(result.manual_overrides.marked_as_eligible.is_empty());
        assert!(result.manual_overrides.marked_as_not_eligible.is_empty());
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
            reason: Some(
                "Eligible: PR found in target branch and no work items to check".to_string(),
            ),
        };

        let analyses = vec![no_work_items_pr];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 0);
        assert_eq!(result.not_merged_prs.len(), 0);
        assert_eq!(result.all_details.len(), 1);
        assert!(result.manual_overrides.marked_as_eligible.is_empty());
        assert!(result.manual_overrides.marked_as_not_eligible.is_empty());
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
            unsure_reason: None,
            reason: Some("Eligible: PR found in target branch (work items not in terminal state but overridden): #1 (Active), #2 (In Progress)".to_string()),
        };

        let analyses = vec![pr_with_non_terminal_work_items];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        // Now this should be eligible since commit is in target branch
        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 0);
        assert_eq!(result.not_merged_prs.len(), 0);
        assert_eq!(result.unsure_details.len(), 0);
        assert_eq!(result.all_details.len(), 1);
        assert!(result.manual_overrides.marked_as_eligible.is_empty());
        assert!(result.manual_overrides.marked_as_not_eligible.is_empty());
    }

    #[tokio::test]
    async fn test_commit_in_target_overrides_work_item_state() {
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

        // Test PR with non-terminal work items but commit in target branch (should be eligible)
        let pr_with_commit_in_target = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "Non-terminal WI but merged", Some("abc123".to_string())),
                work_items: vec![
                    create_test_work_item(1, "Active"),
                    create_test_work_item(2, "In Progress"),
                ],
                selected: false,
            },
            all_work_items_terminal: false,
            terminal_work_items: Vec::new(),
            non_terminal_work_items: vec![
                create_test_work_item(1, "Active"),
                create_test_work_item(2, "In Progress"),
            ],
            commit_in_target: true,
            commit_title_in_target: false,
            commit_id: "abc123".to_string(),
            unsure_reason: None,
            reason: Some("Eligible: PR found in target branch (work items not in terminal state but overridden): #1 (Active), #2 (In Progress)".to_string()),
        };

        // Test PR with terminal work items but NOT in target branch (should be unsure)
        let pr_not_in_target = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(2, "Terminal WI but not merged", Some("def456".to_string())),
                work_items: vec![create_test_work_item(3, "Closed")],
                selected: false,
            },
            all_work_items_terminal: true,
            terminal_work_items: vec![create_test_work_item(3, "Closed")],
            non_terminal_work_items: Vec::new(),
            commit_in_target: false,
            commit_title_in_target: false,
            commit_id: "def456".to_string(),
            unsure_reason: Some(
                "Work items are in terminal state but PR not found in target branch".to_string(),
            ),
            reason: Some(
                "Unsure: Work items are in terminal state but PR not found in target branch"
                    .to_string(),
            ),
        };

        let analyses = vec![pr_with_commit_in_target, pr_not_in_target];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        // First PR should be eligible because commit is in target (overrides work item state)
        // Second PR should be unsure because work items are terminal but commit not in target
        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 1);
        assert_eq!(result.not_merged_prs.len(), 0);
        assert_eq!(result.unsure_details.len(), 1);
        assert_eq!(result.all_details.len(), 2);

        // Verify the eligible PR is the one with commit in target
        assert_eq!(result.eligible_prs[0].pr.id, 1);
        // Verify the unsure PR is the one without commit in target
        assert_eq!(result.unsure_prs[0].pr.id, 2);
        assert!(result.manual_overrides.marked_as_eligible.is_empty());
        assert!(result.manual_overrides.marked_as_not_eligible.is_empty());
    }

    #[tokio::test]
    async fn test_not_merged_reasons() {
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

        // Test PR with non-terminal work items and NOT in target branch (should be not_merged)
        let pr_not_merged_with_wi = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "Not merged with WI", Some("abc123".to_string())),
                work_items: vec![
                    create_test_work_item(1, "Active"),
                    create_test_work_item(2, "In Progress"),
                ],
                selected: false,
            },
            all_work_items_terminal: false,
            terminal_work_items: Vec::new(),
            non_terminal_work_items: vec![
                create_test_work_item(1, "Active"),
                create_test_work_item(2, "In Progress"),
            ],
            commit_in_target: false,
            commit_title_in_target: false,
            commit_id: "abc123".to_string(),
            unsure_reason: None,
            reason: Some("Not merged: Work items not in terminal state and PR not found in target branch: #1 (Active), #2 (In Progress)".to_string()),
        };

        // Test PR with no work items and NOT in target branch (should be not_merged)
        let pr_not_merged_no_wi = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(2, "Not merged no WI", Some("def456".to_string())),
                work_items: Vec::new(),
                selected: false,
            },
            all_work_items_terminal: true, // true because no work items
            terminal_work_items: Vec::new(),
            non_terminal_work_items: Vec::new(),
            commit_in_target: false,
            commit_title_in_target: false,
            commit_id: "def456".to_string(),
            unsure_reason: Some(
                "No work items found and PR not found in target branch".to_string(),
            ),
            reason: Some(
                "Unsure: No work items found and PR not found in target branch".to_string(),
            ),
        };

        let analyses = vec![pr_not_merged_with_wi, pr_not_merged_no_wi];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        // First PR should be not_merged, second should be unsure
        assert_eq!(result.eligible_prs.len(), 0);
        assert_eq!(result.unsure_prs.len(), 1);
        assert_eq!(result.not_merged_prs.len(), 1);
        assert_eq!(result.all_details.len(), 2);
        assert!(result.manual_overrides.marked_as_eligible.is_empty());
        assert!(result.manual_overrides.marked_as_not_eligible.is_empty());

        // Verify the not_merged PR has a proper reason
        let not_merged_detail = result.all_details.iter().find(|d| d.pr.pr.id == 1).unwrap();
        assert!(not_merged_detail.reason.is_some());
        assert!(
            not_merged_detail
                .reason
                .as_ref()
                .unwrap()
                .contains("Not merged")
        );
        assert!(
            not_merged_detail
                .reason
                .as_ref()
                .unwrap()
                .contains("#1 (Active)")
        );
        assert!(
            not_merged_detail
                .reason
                .as_ref()
                .unwrap()
                .contains("#2 (In Progress)")
        );

        // Verify the unsure PR has a proper reason
        let unsure_detail = result.all_details.iter().find(|d| d.pr.pr.id == 2).unwrap();
        assert!(unsure_detail.reason.is_some());
        assert!(unsure_detail.reason.as_ref().unwrap().contains("Unsure"));
    }

    #[tokio::test]
    async fn test_enhanced_pr_detection_details() {
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

        // Test PR found by commit ID
        let pr_found_by_commit = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "Found by commit", Some("abc123".to_string())),
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
            reason: Some("Eligible: Work items in terminal state and PR found in target branch. Detection: Commit 'abc123' found in target branch".to_string()),
        };

        // Test PR found by title pattern
        let pr_found_by_title = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(2, "Found by title", Some("def456".to_string())),
                work_items: vec![create_test_work_item(2, "Closed")],
                selected: false,
            },
            all_work_items_terminal: true,
            terminal_work_items: vec![create_test_work_item(2, "Closed")],
            non_terminal_work_items: Vec::new(),
            commit_in_target: false,
            commit_title_in_target: true,
            commit_id: "def456".to_string(),
            unsure_reason: None,
            reason: Some("Eligible: Work items in terminal state and PR found in target branch. Detection: PR pattern found in commit history (commit ID not directly found)".to_string()),
        };

        // Test PR not found anywhere
        let pr_not_found = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(3, "Not found", Some("ghi789".to_string())),
                work_items: vec![create_test_work_item(3, "Active")],
                selected: false,
            },
            all_work_items_terminal: false,
            terminal_work_items: Vec::new(),
            non_terminal_work_items: vec![create_test_work_item(3, "Active")],
            commit_in_target: false,
            commit_title_in_target: false,
            commit_id: "ghi789".to_string(),
            unsure_reason: None,
            reason: Some("Not merged: Work items not in terminal state and PR not found in target branch: #3 (Active). Detection attempts: commit ID 'ghi789' not found in target, PR title/ID not found in commit history".to_string()),
        };

        let analyses = vec![pr_found_by_commit, pr_found_by_title, pr_not_found];
        let result = analyzer.categorize_prs(analyses, symmetric_diff).unwrap();

        assert_eq!(result.eligible_prs.len(), 2);
        assert_eq!(result.unsure_prs.len(), 0);
        assert_eq!(result.not_merged_prs.len(), 1);
        assert_eq!(result.all_details.len(), 3);
        assert!(result.manual_overrides.marked_as_eligible.is_empty());
        assert!(result.manual_overrides.marked_as_not_eligible.is_empty());

        // Verify detailed reasons include detection information
        let eligible_1 = result.all_details.iter().find(|d| d.pr.pr.id == 1).unwrap();
        assert!(
            eligible_1
                .reason
                .as_ref()
                .unwrap()
                .contains("Detection: Commit 'abc123' found in target branch")
        );

        let eligible_2 = result.all_details.iter().find(|d| d.pr.pr.id == 2).unwrap();
        assert!(
            eligible_2
                .reason
                .as_ref()
                .unwrap()
                .contains("Detection: PR pattern found in commit history")
        );

        let not_merged = result.all_details.iter().find(|d| d.pr.pr.id == 3).unwrap();
        assert!(
            not_merged
                .reason
                .as_ref()
                .unwrap()
                .contains("Detection attempts: commit ID 'ghi789' not found")
        );
        assert!(
            not_merged
                .reason
                .as_ref()
                .unwrap()
                .contains("PR title/ID not found in commit history")
        );
    }

    #[tokio::test]
    async fn test_manual_overrides() {
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

        // PR that would naturally be eligible but manually marked as not eligible
        let naturally_eligible_pr = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(1, "Naturally eligible", Some("abc123".to_string())),
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
            reason: Some("Eligible: Work items in terminal state and PR found in target branch".to_string()),
        };

        // PR that would naturally not be eligible but manually marked as eligible
        let naturally_not_eligible_pr = PRAnalysisResult {
            pr: PullRequestWithWorkItems {
                pr: create_test_pr(2, "Naturally not eligible", Some("def456".to_string())),
                work_items: vec![create_test_work_item(2, "Active")],
                selected: false,
            },
            all_work_items_terminal: false,
            terminal_work_items: Vec::new(),
            non_terminal_work_items: vec![create_test_work_item(2, "Active")],
            commit_in_target: false,
            commit_title_in_target: false,
            commit_id: "def456".to_string(),
            unsure_reason: None,
            reason: Some("Not merged: Work items not in terminal state and PR not found in target branch".to_string()),
        };

        // Create manual overrides
        let mut manual_overrides = crate::models::ManualOverrides::default();
        manual_overrides.marked_as_not_eligible.insert(1); // PR 1 manually marked not eligible
        manual_overrides.marked_as_eligible.insert(2); // PR 2 manually marked eligible

        let analyses = vec![naturally_eligible_pr, naturally_not_eligible_pr];
        let result = analyzer.categorize_prs_with_overrides(analyses, symmetric_diff, manual_overrides.clone()).unwrap();

        // Verify manual overrides work
        assert_eq!(result.eligible_prs.len(), 1);
        assert_eq!(result.unsure_prs.len(), 0);
        assert_eq!(result.not_merged_prs.len(), 1);
        assert_eq!(result.all_details.len(), 2);

        // PR 1 should be in not_merged despite being naturally eligible (manual override)
        assert_eq!(result.not_merged_prs[0].pr.id, 1);
        // PR 2 should be in eligible despite being naturally not eligible (manual override)
        assert_eq!(result.eligible_prs[0].pr.id, 2);

        // Verify manual overrides are preserved
        assert!(result.manual_overrides.marked_as_not_eligible.contains(&1));
        assert!(result.manual_overrides.marked_as_eligible.contains(&2));
        assert_eq!(result.manual_overrides.marked_as_not_eligible.len(), 1);
        assert_eq!(result.manual_overrides.marked_as_eligible.len(), 1);
    }
}
