//! Work item grouping operations for PR selection.
//!
//! This module provides functionality to group PRs that share work items,
//! detect when selecting a PR would leave related PRs unselected, and
//! allow batch selection of related PRs.

use crate::models::PullRequestWithWorkItems;
use std::collections::{HashMap, HashSet};

/// Index mapping work item IDs to PR indices and vice versa.
///
/// This structure enables efficient lookup of PRs that share work items,
/// which is used to warn users when they select a PR without including
/// other PRs connected to the same work item.
#[derive(Debug, Clone, Default)]
pub struct WorkItemPrIndex {
    /// Map from work item ID to list of PR indices that reference it
    work_item_to_prs: HashMap<i32, Vec<usize>>,
    /// Map from PR index to list of work item IDs it references
    pr_to_work_items: HashMap<usize, Vec<i32>>,
}

impl WorkItemPrIndex {
    /// Build an index from a list of PRs with work items.
    ///
    /// # Arguments
    ///
    /// * `prs` - The list of PRs to index
    ///
    /// # Returns
    ///
    /// A new `WorkItemPrIndex` with bidirectional mappings.
    pub fn build(prs: &[PullRequestWithWorkItems]) -> Self {
        let mut work_item_to_prs: HashMap<i32, Vec<usize>> = HashMap::new();
        let mut pr_to_work_items: HashMap<usize, Vec<i32>> = HashMap::new();

        for (pr_index, pr) in prs.iter().enumerate() {
            let work_item_ids: Vec<i32> = pr.work_items.iter().map(|wi| wi.id).collect();

            if !work_item_ids.is_empty() {
                pr_to_work_items.insert(pr_index, work_item_ids.clone());

                for wi_id in work_item_ids {
                    work_item_to_prs.entry(wi_id).or_default().push(pr_index);
                }
            }
        }

        Self {
            work_item_to_prs,
            pr_to_work_items,
        }
    }

    /// Get all PR indices that share any work item with the given PR.
    ///
    /// This returns all PRs that have at least one work item in common
    /// with the specified PR, excluding the PR itself.
    ///
    /// # Arguments
    ///
    /// * `pr_index` - The index of the PR to find related PRs for
    ///
    /// # Returns
    ///
    /// A sorted, deduplicated vector of PR indices that share work items.
    pub fn get_related_pr_indices(&self, pr_index: usize) -> Vec<usize> {
        let mut related: HashSet<usize> = HashSet::new();

        if let Some(work_item_ids) = self.pr_to_work_items.get(&pr_index) {
            for wi_id in work_item_ids {
                if let Some(pr_indices) = self.work_item_to_prs.get(wi_id) {
                    for &idx in pr_indices {
                        if idx != pr_index {
                            related.insert(idx);
                        }
                    }
                }
            }
        }

        let mut result: Vec<usize> = related.into_iter().collect();
        result.sort_unstable();
        result
    }

    /// Get all PR indices that reference a specific work item.
    ///
    /// # Arguments
    ///
    /// * `work_item_id` - The ID of the work item
    ///
    /// # Returns
    ///
    /// A slice of PR indices, or an empty slice if the work item is not found.
    pub fn get_prs_for_work_item(&self, work_item_id: i32) -> &[usize] {
        self.work_item_to_prs
            .get(&work_item_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get the work item IDs shared between two PRs.
    ///
    /// # Arguments
    ///
    /// * `pr_index_a` - The index of the first PR
    /// * `pr_index_b` - The index of the second PR
    ///
    /// # Returns
    ///
    /// A vector of work item IDs that both PRs reference.
    pub fn get_shared_work_items(&self, pr_index_a: usize, pr_index_b: usize) -> Vec<i32> {
        let Some(work_items_a) = self.pr_to_work_items.get(&pr_index_a) else {
            return Vec::new();
        };
        let Some(work_items_b) = self.pr_to_work_items.get(&pr_index_b) else {
            return Vec::new();
        };

        let set_a: HashSet<i32> = work_items_a.iter().copied().collect();
        work_items_b
            .iter()
            .filter(|id| set_a.contains(id))
            .copied()
            .collect()
    }

    /// Get the work item IDs for a specific PR.
    ///
    /// # Arguments
    ///
    /// * `pr_index` - The index of the PR
    ///
    /// # Returns
    ///
    /// A slice of work item IDs, or an empty slice if the PR has no work items.
    pub fn get_work_items_for_pr(&self, pr_index: usize) -> &[i32] {
        self.pr_to_work_items
            .get(&pr_index)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Check if a work item is shared by multiple PRs.
    ///
    /// # Arguments
    ///
    /// * `work_item_id` - The ID of the work item
    ///
    /// # Returns
    ///
    /// `true` if the work item is referenced by more than one PR.
    pub fn is_shared_work_item(&self, work_item_id: i32) -> bool {
        self.work_item_to_prs
            .get(&work_item_id)
            .map(|prs| prs.len() > 1)
            .unwrap_or(false)
    }
}

/// Warning information when selecting a PR that has unselected related PRs.
#[derive(Debug, Clone)]
pub struct SelectionWarning {
    /// The PR index being selected
    pub selected_pr_index: usize,
    /// Related PR indices that are not currently selected
    pub unselected_related_prs: Vec<usize>,
    /// Map of work item ID to the PR indices that share it (excluding selected PR)
    pub shared_work_items: HashMap<i32, Vec<usize>>,
}

/// Check if selecting a PR would leave related PRs unselected.
///
/// This function checks whether the PR being selected shares work items with
/// other PRs that are not currently selected. If so, it returns a warning
/// with details about the related PRs.
///
/// # Arguments
///
/// * `prs` - The list of all PRs
/// * `index` - The work item to PR index
/// * `pr_index` - The index of the PR being selected
///
/// # Returns
///
/// `Some(SelectionWarning)` if there are unselected related PRs, `None` otherwise.
pub fn check_selection_warning(
    prs: &[PullRequestWithWorkItems],
    index: &WorkItemPrIndex,
    pr_index: usize,
) -> Option<SelectionWarning> {
    // Get all PRs related to this one
    let related_indices = index.get_related_pr_indices(pr_index);

    if related_indices.is_empty() {
        return None;
    }

    // Find unselected related PRs
    let unselected_related: Vec<usize> = related_indices
        .into_iter()
        .filter(|&idx| prs.get(idx).map(|pr| !pr.selected).unwrap_or(false))
        .collect();

    if unselected_related.is_empty() {
        return None;
    }

    // Build map of shared work items to unselected PR indices
    let mut shared_work_items: HashMap<i32, Vec<usize>> = HashMap::new();
    let work_items = index.get_work_items_for_pr(pr_index);

    for &wi_id in work_items {
        let prs_for_wi: Vec<usize> = index
            .get_prs_for_work_item(wi_id)
            .iter()
            .filter(|&&idx| idx != pr_index && unselected_related.contains(&idx))
            .copied()
            .collect();

        if !prs_for_wi.is_empty() {
            shared_work_items.insert(wi_id, prs_for_wi);
        }
    }

    Some(SelectionWarning {
        selected_pr_index: pr_index,
        unselected_related_prs: unselected_related,
        shared_work_items,
    })
}

/// Get work item title from a PR list by work item ID.
///
/// # Arguments
///
/// * `prs` - The list of PRs to search
/// * `work_item_id` - The ID of the work item
///
/// # Returns
///
/// The work item title if found, or a placeholder string.
pub fn get_work_item_title(prs: &[PullRequestWithWorkItems], work_item_id: i32) -> String {
    for pr in prs {
        for wi in &pr.work_items {
            if wi.id == work_item_id {
                return wi
                    .fields
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("Work Item #{}", work_item_id));
            }
        }
    }
    format!("Work Item #{}", work_item_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreatedBy, PullRequest, WorkItem, WorkItemFields};

    fn create_pr_with_work_item_ids(
        pr_id: i32,
        work_item_ids: Vec<i32>,
        selected: bool,
    ) -> PullRequestWithWorkItems {
        let work_items = work_item_ids
            .into_iter()
            .map(|wi_id| WorkItem {
                id: wi_id,
                fields: WorkItemFields {
                    title: Some(format!("Work Item {}", wi_id)),
                    state: Some("Active".to_string()),
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
                id: pr_id,
                title: format!("PR {}", pr_id),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "user".to_string(),
                },
                labels: None,
                last_merge_commit: None,
            },
            work_items,
            selected,
        }
    }

    /// # Build Index - Empty PRs
    ///
    /// Verifies that building an index from empty PR list works.
    ///
    /// ## Test Scenario
    /// - Empty PR list
    ///
    /// ## Expected Outcome
    /// - Empty index with no mappings
    #[test]
    fn test_build_index_empty_prs() {
        let prs: Vec<PullRequestWithWorkItems> = vec![];
        let index = WorkItemPrIndex::build(&prs);

        assert!(index.work_item_to_prs.is_empty());
        assert!(index.pr_to_work_items.is_empty());
    }

    /// # Build Index - No Shared Work Items
    ///
    /// Verifies index building when PRs have distinct work items.
    ///
    /// ## Test Scenario
    /// - Two PRs each with unique work items
    ///
    /// ## Expected Outcome
    /// - Each work item maps to exactly one PR
    #[test]
    fn test_build_index_no_shared_work_items() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100, 101], false),
            create_pr_with_work_item_ids(2, vec![200, 201], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert_eq!(index.get_prs_for_work_item(100), &[0]);
        assert_eq!(index.get_prs_for_work_item(101), &[0]);
        assert_eq!(index.get_prs_for_work_item(200), &[1]);
        assert_eq!(index.get_prs_for_work_item(201), &[1]);
        assert!(index.get_related_pr_indices(0).is_empty());
        assert!(index.get_related_pr_indices(1).is_empty());
    }

    /// # Build Index - Single Shared Work Item
    ///
    /// Verifies index building when two PRs share one work item.
    ///
    /// ## Test Scenario
    /// - Two PRs sharing work item 100
    ///
    /// ## Expected Outcome
    /// - Work item 100 maps to both PR indices
    /// - Each PR is related to the other
    #[test]
    fn test_build_index_single_shared_work_item() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![100], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert_eq!(index.get_prs_for_work_item(100), &[0, 1]);
        assert_eq!(index.get_related_pr_indices(0), vec![1]);
        assert_eq!(index.get_related_pr_indices(1), vec![0]);
    }

    /// # Build Index - Multiple Shared Work Items
    ///
    /// Verifies index building with complex sharing patterns.
    ///
    /// ## Test Scenario
    /// - Three PRs with overlapping work items
    /// - PR0: [100, 101], PR1: [100, 102], PR2: [101, 102]
    ///
    /// ## Expected Outcome
    /// - Each PR is related to both other PRs
    /// - Work item mappings are correct
    #[test]
    fn test_build_index_multiple_shared_work_items() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100, 101], false),
            create_pr_with_work_item_ids(2, vec![100, 102], false),
            create_pr_with_work_item_ids(3, vec![101, 102], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        // PR 0 shares 100 with PR 1, and 101 with PR 2
        let mut related_0 = index.get_related_pr_indices(0);
        related_0.sort();
        assert_eq!(related_0, vec![1, 2]);

        // PR 1 shares 100 with PR 0, and 102 with PR 2
        let mut related_1 = index.get_related_pr_indices(1);
        related_1.sort();
        assert_eq!(related_1, vec![0, 2]);

        // PR 2 shares 101 with PR 0, and 102 with PR 1
        let mut related_2 = index.get_related_pr_indices(2);
        related_2.sort();
        assert_eq!(related_2, vec![0, 1]);
    }

    /// # Get Related PR Indices - No Relations
    ///
    /// Verifies get_related_pr_indices returns empty for isolated PRs.
    ///
    /// ## Test Scenario
    /// - PR with unique work items
    ///
    /// ## Expected Outcome
    /// - Empty related indices
    #[test]
    fn test_get_related_pr_indices_no_relations() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![200], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert!(index.get_related_pr_indices(0).is_empty());
        assert!(index.get_related_pr_indices(1).is_empty());
    }

    /// # Get Related PR Indices - With Relations
    ///
    /// Verifies get_related_pr_indices returns correct related PRs.
    ///
    /// ## Test Scenario
    /// - Three PRs where PR 0 and PR 1 share work item 100
    /// - PR 2 is independent
    ///
    /// ## Expected Outcome
    /// - PR 0 is related to PR 1
    /// - PR 1 is related to PR 0
    /// - PR 2 has no relations
    #[test]
    fn test_get_related_pr_indices_with_relations() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![100], false),
            create_pr_with_work_item_ids(3, vec![200], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert_eq!(index.get_related_pr_indices(0), vec![1]);
        assert_eq!(index.get_related_pr_indices(1), vec![0]);
        assert!(index.get_related_pr_indices(2).is_empty());
    }

    /// # Get Shared Work Items
    ///
    /// Verifies get_shared_work_items returns correct shared IDs.
    ///
    /// ## Test Scenario
    /// - Two PRs sharing work item 100, each with additional unique items
    ///
    /// ## Expected Outcome
    /// - Returns only the shared work item ID
    #[test]
    fn test_get_shared_work_items() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100, 101], false),
            create_pr_with_work_item_ids(2, vec![100, 102], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert_eq!(index.get_shared_work_items(0, 1), vec![100]);
        assert_eq!(index.get_shared_work_items(1, 0), vec![100]);
    }

    /// # Get Shared Work Items - No Overlap
    ///
    /// Verifies get_shared_work_items returns empty for non-overlapping PRs.
    ///
    /// ## Test Scenario
    /// - Two PRs with distinct work items
    ///
    /// ## Expected Outcome
    /// - Empty result
    #[test]
    fn test_get_shared_work_items_no_overlap() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![200], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert!(index.get_shared_work_items(0, 1).is_empty());
    }

    /// # Check Selection Warning - No Warning
    ///
    /// Verifies no warning when selecting PR with no related PRs.
    ///
    /// ## Test Scenario
    /// - PR with unique work items
    ///
    /// ## Expected Outcome
    /// - Returns None
    #[test]
    fn test_check_selection_warning_no_warning() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![200], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        let warning = check_selection_warning(&prs, &index, 0);
        assert!(warning.is_none());
    }

    /// # Check Selection Warning - With Unselected Related
    ///
    /// Verifies warning when selecting PR with unselected related PRs.
    ///
    /// ## Test Scenario
    /// - Two PRs sharing work item 100
    /// - Neither is selected
    /// - Selecting PR 0
    ///
    /// ## Expected Outcome
    /// - Returns warning with PR 1 as unselected related
    #[test]
    fn test_check_selection_warning_with_unselected_related() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![100], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        let warning = check_selection_warning(&prs, &index, 0);
        assert!(warning.is_some());

        let warning = warning.unwrap();
        assert_eq!(warning.selected_pr_index, 0);
        assert_eq!(warning.unselected_related_prs, vec![1]);
        assert!(warning.shared_work_items.contains_key(&100));
        assert_eq!(warning.shared_work_items.get(&100), Some(&vec![1]));
    }

    /// # Check Selection Warning - All Related Selected
    ///
    /// Verifies no warning when all related PRs are already selected.
    ///
    /// ## Test Scenario
    /// - Two PRs sharing work item 100
    /// - PR 1 is already selected
    /// - Selecting PR 0
    ///
    /// ## Expected Outcome
    /// - Returns None
    #[test]
    fn test_check_selection_warning_all_related_selected() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![100], true), // Already selected
        ];
        let index = WorkItemPrIndex::build(&prs);

        let warning = check_selection_warning(&prs, &index, 0);
        assert!(warning.is_none());
    }

    /// # Check Selection Warning - Multiple Unselected
    ///
    /// Verifies warning includes all unselected related PRs.
    ///
    /// ## Test Scenario
    /// - Three PRs all sharing work item 100
    /// - None selected
    /// - Selecting PR 0
    ///
    /// ## Expected Outcome
    /// - Warning includes PR 1 and PR 2
    #[test]
    fn test_check_selection_warning_multiple_unselected() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![100], false),
            create_pr_with_work_item_ids(3, vec![100], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        let warning = check_selection_warning(&prs, &index, 0);
        assert!(warning.is_some());

        let warning = warning.unwrap();
        assert_eq!(warning.unselected_related_prs, vec![1, 2]);
    }

    /// # Check Selection Warning - Some Related Selected
    ///
    /// Verifies warning only includes unselected related PRs.
    ///
    /// ## Test Scenario
    /// - Three PRs sharing work item 100
    /// - PR 2 is already selected
    /// - Selecting PR 0
    ///
    /// ## Expected Outcome
    /// - Warning includes only PR 1
    #[test]
    fn test_check_selection_warning_some_related_selected() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100], false),
            create_pr_with_work_item_ids(2, vec![100], false),
            create_pr_with_work_item_ids(3, vec![100], true), // Already selected
        ];
        let index = WorkItemPrIndex::build(&prs);

        let warning = check_selection_warning(&prs, &index, 0);
        assert!(warning.is_some());

        let warning = warning.unwrap();
        assert_eq!(warning.unselected_related_prs, vec![1]);
    }

    /// # Is Shared Work Item
    ///
    /// Verifies is_shared_work_item correctly identifies shared work items.
    ///
    /// ## Test Scenario
    /// - Work item 100 shared by two PRs
    /// - Work item 200 only on one PR
    ///
    /// ## Expected Outcome
    /// - 100 is shared, 200 is not
    #[test]
    fn test_is_shared_work_item() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![100, 200], false),
            create_pr_with_work_item_ids(2, vec![100], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert!(index.is_shared_work_item(100));
        assert!(!index.is_shared_work_item(200));
        assert!(!index.is_shared_work_item(999)); // Non-existent
    }

    /// # PR Without Work Items
    ///
    /// Verifies PRs without work items are handled correctly.
    ///
    /// ## Test Scenario
    /// - PR with no work items
    ///
    /// ## Expected Outcome
    /// - No mappings for that PR
    /// - No warnings when selecting
    #[test]
    fn test_pr_without_work_items() {
        let prs = vec![
            create_pr_with_work_item_ids(1, vec![], false),
            create_pr_with_work_item_ids(2, vec![100], false),
        ];
        let index = WorkItemPrIndex::build(&prs);

        assert!(index.get_work_items_for_pr(0).is_empty());
        assert!(index.get_related_pr_indices(0).is_empty());
        assert!(check_selection_warning(&prs, &index, 0).is_none());
    }

    /// # Get Work Item Title
    ///
    /// Verifies work item title lookup works correctly.
    ///
    /// ## Test Scenario
    /// - PR with work item that has a title
    ///
    /// ## Expected Outcome
    /// - Returns correct title
    /// - Returns placeholder for missing work items
    #[test]
    fn test_get_work_item_title() {
        let prs = vec![create_pr_with_work_item_ids(1, vec![100], false)];

        assert_eq!(get_work_item_title(&prs, 100), "Work Item 100");
        assert_eq!(get_work_item_title(&prs, 999), "Work Item #999");
    }
}
