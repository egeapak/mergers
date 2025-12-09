//! Model mapping utilities for converting between azure_devops_rust_api types and our domain types.
//!
//! This module provides conversion implementations (`From` traits) to map the auto-generated
//! types from the azure_devops_rust_api crate to our simpler, purpose-built domain models.

use crate::models::{
    CreatedBy, Label, MergeCommit, PullRequest, RepoDetails, WorkItem, WorkItemFieldChange,
    WorkItemFields, WorkItemHistory, WorkItemHistoryFields,
};
use azure_devops_rust_api::git::models as git_models;
use azure_devops_rust_api::wit::models as wit_models;

/// Convert azure_devops_rust_api GitPullRequest to our PullRequest model.
impl From<git_models::GitPullRequest> for PullRequest {
    fn from(pr: git_models::GitPullRequest) -> Self {
        PullRequest {
            id: pr.pull_request_id,
            title: pr.title.unwrap_or_default(),
            closed_date: pr.closed_date.map(|d| {
                // Convert time::OffsetDateTime to RFC3339 string for compatibility
                d.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_else(|_| d.to_string())
            }),
            created_by: CreatedBy {
                display_name: pr
                    .created_by
                    .graph_subject_base
                    .display_name
                    .unwrap_or_default(),
            },
            last_merge_commit: pr.last_merge_commit.map(|c| MergeCommit {
                commit_id: c.commit_id.unwrap_or_default(),
            }),
            labels: if pr.labels.is_empty() {
                None
            } else {
                Some(
                    pr.labels
                        .into_iter()
                        .map(|l| Label {
                            name: l.name.unwrap_or_default(),
                        })
                        .collect(),
                )
            },
        }
    }
}

/// Convert azure_devops_rust_api WorkItem to our WorkItem model.
impl From<wit_models::WorkItem> for WorkItem {
    fn from(wi: wit_models::WorkItem) -> Self {
        // The fields are stored as a serde_json::Value (object/map)
        let fields = &wi.fields;

        WorkItem {
            id: wi.id,
            fields: WorkItemFields {
                title: fields
                    .get("System.Title")
                    .and_then(|v| v.as_str().map(String::from)),
                state: fields
                    .get("System.State")
                    .and_then(|v| v.as_str().map(String::from)),
                work_item_type: fields
                    .get("System.WorkItemType")
                    .and_then(|v| v.as_str().map(String::from)),
                assigned_to: fields.get("System.AssignedTo").and_then(|v| {
                    v.get("displayName")
                        .and_then(|name| name.as_str())
                        .map(|name| CreatedBy {
                            display_name: name.to_string(),
                        })
                }),
                iteration_path: fields
                    .get("System.IterationPath")
                    .and_then(|v| v.as_str().map(String::from)),
                description: fields
                    .get("System.Description")
                    .and_then(|v| v.as_str().map(String::from)),
                repro_steps: fields
                    .get("Microsoft.VSTS.TCM.ReproSteps")
                    .and_then(|v| v.as_str().map(String::from)),
            },
            history: vec![], // History is populated separately
        }
    }
}

/// Convert azure_devops_rust_api WorkItemUpdate to our WorkItemHistory model.
impl From<wit_models::WorkItemUpdate> for WorkItemHistory {
    fn from(update: wit_models::WorkItemUpdate) -> Self {
        let fields_value = update.fields.as_ref();

        let fields = fields_value.map(|f| WorkItemHistoryFields {
            state: f.get("System.State").and_then(|v| {
                v.get("newValue")
                    .and_then(|nv| nv.as_str())
                    .map(|s| WorkItemFieldChange {
                        new_value: Some(s.to_string()),
                    })
            }),
            changed_date: f.get("System.ChangedDate").and_then(|v| {
                v.get("newValue")
                    .and_then(|nv| nv.as_str())
                    .map(|s| WorkItemFieldChange {
                        new_value: Some(s.to_string()),
                    })
            }),
        });

        WorkItemHistory {
            rev: update.rev.unwrap_or_default(),
            revised_date: update
                .revised_date
                .map(|d| {
                    d.format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_else(|_| d.to_string())
                })
                .unwrap_or_default(),
            fields,
        }
    }
}

/// Convert azure_devops_rust_api GitRepository to our RepoDetails model.
impl From<git_models::GitRepository> for RepoDetails {
    fn from(repo: git_models::GitRepository) -> Self {
        RepoDetails {
            ssh_url: repo.ssh_url.unwrap_or_default(),
        }
    }
}

/// Helper function to extract work item ID from a ResourceRef URL.
///
/// Azure DevOps returns work item references as URLs like:
/// `https://dev.azure.com/org/project/_apis/wit/workItems/12345`
///
/// This function extracts the numeric ID from such URLs.
pub fn extract_work_item_id(url: &str) -> Option<i32> {
    url.rsplit('/').next().and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Work Item ID Extraction from URL
    ///
    /// Tests extraction of work item IDs from Azure DevOps URLs.
    ///
    /// ## Test Scenario
    /// - Provides various work item URLs
    /// - Extracts the numeric ID from each URL
    ///
    /// ## Expected Outcome
    /// - Correct ID is extracted from valid URLs
    /// - None is returned for invalid URLs
    #[test]
    fn test_extract_work_item_id() {
        assert_eq!(
            extract_work_item_id("https://dev.azure.com/org/project/_apis/wit/workItems/12345"),
            Some(12345)
        );
        assert_eq!(extract_work_item_id("invalid-url"), None);
        assert_eq!(extract_work_item_id(""), None);
    }
}
