//! PR selection operations based on work item states.
//!
//! This module provides functions to filter and select pull requests based on
//! the states of their associated work items. This is primarily used for
//! non-interactive mode where PRs are automatically selected.

use crate::models::PullRequestWithWorkItems;

/// Filters PRs to only those where ALL work items are in one of the specified states.
///
/// # Rules
///
/// 1. PR must have at least one work item
/// 2. ALL work items must be in one of the specified states (case-insensitive)
///
/// # Arguments
///
/// * `prs` - The list of PRs to filter
/// * `states` - The work item states to match against
///
/// # Returns
///
/// A vector of references to PRs that match the criteria.
///
/// # Example
///
/// ```ignore
/// let matching = filter_prs_by_work_item_states(&prs, &["Ready", "Approved"]);
/// ```
pub fn filter_prs_by_work_item_states<'a>(
    prs: &'a [PullRequestWithWorkItems],
    states: &[String],
) -> Vec<&'a PullRequestWithWorkItems> {
    // Normalize states to lowercase for case-insensitive matching
    let normalized_states: Vec<String> = states.iter().map(|s| s.to_lowercase()).collect();

    prs.iter()
        .filter(|pr| {
            // Must have at least one work item
            if pr.work_items.is_empty() {
                return false;
            }

            // All work items must be in one of the specified states
            pr.work_items.iter().all(|wi| {
                wi.fields
                    .state
                    .as_ref()
                    .map(|state| normalized_states.contains(&state.to_lowercase()))
                    .unwrap_or(false)
            })
        })
        .collect()
}

/// Selects PRs in-place where ALL work items are in one of the specified states.
///
/// This modifies the `selected` field of each PR based on whether it matches
/// the criteria.
///
/// # Rules
///
/// 1. PR must have at least one work item
/// 2. ALL work items must be in one of the specified states (case-insensitive)
/// 3. PRs not matching the criteria are deselected
///
/// # Arguments
///
/// * `prs` - The list of PRs to modify
/// * `states` - The work item states to match against
///
/// # Returns
///
/// The count of PRs that were selected.
pub fn select_prs_by_work_item_states(
    prs: &mut [PullRequestWithWorkItems],
    states: &[String],
) -> usize {
    // Normalize states to lowercase for case-insensitive matching
    let normalized_states: Vec<String> = states.iter().map(|s| s.to_lowercase()).collect();

    let mut selected_count = 0;

    for pr in prs.iter_mut() {
        let should_select = if pr.work_items.is_empty() {
            // Must have at least one work item
            false
        } else {
            // All work items must be in one of the specified states
            pr.work_items.iter().all(|wi| {
                wi.fields
                    .state
                    .as_ref()
                    .map(|state| normalized_states.contains(&state.to_lowercase()))
                    .unwrap_or(false)
            })
        };

        pr.selected = should_select;
        if should_select {
            selected_count += 1;
        }
    }

    selected_count
}

/// Parses a comma-separated string of work item states.
///
/// # Arguments
///
/// * `states_str` - A comma-separated string like "Ready,Approved,Done"
///
/// # Returns
///
/// A vector of trimmed, non-empty state strings.
pub fn parse_work_item_states(states_str: &str) -> Vec<String> {
    states_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreatedBy, PullRequest, WorkItem, WorkItemFields};

    fn create_pr_with_work_items(
        id: i32,
        work_items: Vec<(&str, Option<&str>)>,
    ) -> PullRequestWithWorkItems {
        let work_items = work_items
            .into_iter()
            .enumerate()
            .map(|(idx, (title, state))| WorkItem {
                id: idx as i32 + 1,
                fields: WorkItemFields {
                    title: Some(title.to_string()),
                    state: state.map(|s| s.to_string()),
                    work_item_type: Some("User Story".to_string()),
                    assigned_to: None,
                    iteration_path: None,
                    description: None,
                    repro_steps: None,
                    state_color: None,
                },
                history: Vec::new(),
            })
            .collect();

        PullRequestWithWorkItems {
            pr: PullRequest {
                id,
                title: format!("PR {}", id),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "user".to_string(),
                },
                labels: None,
                last_merge_commit: None,
            },
            work_items,
            selected: false,
        }
    }

    /// # Filter PRs - All Work Items Match
    ///
    /// Verifies that PRs are included when all work items match.
    ///
    /// ## Test Scenario
    /// - Creates PR with two work items in "Ready" state
    /// - Filters by "Ready" state
    ///
    /// ## Expected Outcome
    /// - PR is included in results
    #[test]
    fn test_filter_all_work_items_match() {
        let prs = vec![create_pr_with_work_items(
            1,
            vec![("WI 1", Some("Ready")), ("WI 2", Some("Ready"))],
        )];
        let states = vec!["Ready".to_string()];

        let result = filter_prs_by_work_item_states(&prs, &states);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pr.id, 1);
    }

    /// # Filter PRs - Some Work Items Don't Match
    ///
    /// Verifies that PRs are excluded when not all work items match.
    ///
    /// ## Test Scenario
    /// - Creates PR with one "Ready" and one "Active" work item
    /// - Filters by "Ready" state only
    ///
    /// ## Expected Outcome
    /// - PR is excluded from results
    #[test]
    fn test_filter_some_work_items_dont_match() {
        let prs = vec![create_pr_with_work_items(
            1,
            vec![("WI 1", Some("Ready")), ("WI 2", Some("Active"))],
        )];
        let states = vec!["Ready".to_string()];

        let result = filter_prs_by_work_item_states(&prs, &states);
        assert!(result.is_empty());
    }

    /// # Filter PRs - No Work Items
    ///
    /// Verifies that PRs without work items are excluded.
    ///
    /// ## Test Scenario
    /// - Creates PR with no work items
    /// - Filters by "Ready" state
    ///
    /// ## Expected Outcome
    /// - PR is excluded from results
    #[test]
    fn test_filter_no_work_items() {
        let prs = vec![create_pr_with_work_items(1, vec![])];
        let states = vec!["Ready".to_string()];

        let result = filter_prs_by_work_item_states(&prs, &states);
        assert!(result.is_empty());
    }

    /// # Filter PRs - Case Insensitive
    ///
    /// Verifies that state matching is case-insensitive.
    ///
    /// ## Test Scenario
    /// - Creates PR with work item in "ready" (lowercase)
    /// - Filters by "READY" (uppercase)
    ///
    /// ## Expected Outcome
    /// - PR is included in results
    #[test]
    fn test_filter_case_insensitive() {
        let prs = vec![create_pr_with_work_items(1, vec![("WI 1", Some("ready"))])];
        let states = vec!["READY".to_string()];

        let result = filter_prs_by_work_item_states(&prs, &states);
        assert_eq!(result.len(), 1);
    }

    /// # Filter PRs - Multiple States
    ///
    /// Verifies that PRs match when work items are in different allowed states.
    ///
    /// ## Test Scenario
    /// - Creates PR with "Ready" and "Approved" work items
    /// - Filters by both "Ready" and "Approved" states
    ///
    /// ## Expected Outcome
    /// - PR is included in results
    #[test]
    fn test_filter_multiple_states() {
        let prs = vec![create_pr_with_work_items(
            1,
            vec![("WI 1", Some("Ready")), ("WI 2", Some("Approved"))],
        )];
        let states = vec!["Ready".to_string(), "Approved".to_string()];

        let result = filter_prs_by_work_item_states(&prs, &states);
        assert_eq!(result.len(), 1);
    }

    /// # Filter PRs - Work Item Without State
    ///
    /// Verifies that work items without a state cause PR exclusion.
    ///
    /// ## Test Scenario
    /// - Creates PR with one work item missing state
    /// - Filters by "Ready" state
    ///
    /// ## Expected Outcome
    /// - PR is excluded from results
    #[test]
    fn test_filter_work_item_no_state() {
        let prs = vec![create_pr_with_work_items(
            1,
            vec![("WI 1", Some("Ready")), ("WI 2", None)],
        )];
        let states = vec!["Ready".to_string()];

        let result = filter_prs_by_work_item_states(&prs, &states);
        assert!(result.is_empty());
    }

    /// # Select PRs In-Place
    ///
    /// Verifies that select_prs_by_work_item_states modifies PRs correctly.
    ///
    /// ## Test Scenario
    /// - Creates multiple PRs with different work item states
    /// - Selects by "Ready" state
    ///
    /// ## Expected Outcome
    /// - Only matching PRs are selected
    /// - Returns correct count
    #[test]
    fn test_select_prs_in_place() {
        let mut prs = vec![
            create_pr_with_work_items(1, vec![("WI 1", Some("Ready"))]),
            create_pr_with_work_items(2, vec![("WI 2", Some("Active"))]),
            create_pr_with_work_items(3, vec![("WI 3", Some("Ready"))]),
        ];
        let states = vec!["Ready".to_string()];

        let count = select_prs_by_work_item_states(&mut prs, &states);

        assert_eq!(count, 2);
        assert!(prs[0].selected);
        assert!(!prs[1].selected);
        assert!(prs[2].selected);
    }

    /// # Parse Work Item States
    ///
    /// Verifies that comma-separated states are parsed correctly.
    ///
    /// ## Test Scenario
    /// - Parses various state strings
    ///
    /// ## Expected Outcome
    /// - States are split, trimmed, and empty entries removed
    #[test]
    fn test_parse_work_item_states() {
        assert_eq!(
            parse_work_item_states("Ready,Approved,Done"),
            vec!["Ready", "Approved", "Done"]
        );
        assert_eq!(
            parse_work_item_states("Ready, Approved, Done"),
            vec!["Ready", "Approved", "Done"]
        );
        assert_eq!(parse_work_item_states("Ready,,Done"), vec!["Ready", "Done"]);
        assert_eq!(parse_work_item_states(""), Vec::<String>::new());
        assert_eq!(parse_work_item_states("  "), Vec::<String>::new());
    }

    /// # Multiple PRs Mixed Results
    ///
    /// Verifies filtering with multiple PRs having different eligibility.
    ///
    /// ## Test Scenario
    /// - Creates 4 PRs with various work item configurations
    /// - Filters by "Ready" and "Approved" states
    ///
    /// ## Expected Outcome
    /// - Only PRs where all work items match are included
    #[test]
    fn test_filter_multiple_prs_mixed() {
        let prs = vec![
            create_pr_with_work_items(1, vec![("WI 1", Some("Ready"))]),
            create_pr_with_work_items(2, vec![("WI 2", Some("Active"))]),
            create_pr_with_work_items(3, vec![("WI 3", Some("Ready")), ("WI 4", Some("Approved"))]),
            create_pr_with_work_items(4, vec![]), // No work items
        ];
        let states = vec!["Ready".to_string(), "Approved".to_string()];

        let result = filter_prs_by_work_item_states(&prs, &states);

        assert_eq!(result.len(), 2);
        let ids: Vec<i32> = result.iter().map(|pr| pr.pr.id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&3));
    }
}
