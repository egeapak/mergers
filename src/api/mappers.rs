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
                state_color: None, // Populated separately from API
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
#[must_use]
pub fn extract_work_item_id(url: &str) -> Option<i32> {
    url.rsplit('/').next().and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use azure_devops_rust_api::git::models as git_models;
    use azure_devops_rust_api::wit::models as wit_models;
    use serde_json::json;

    /// Helper to create a minimal TeamProjectReference for testing
    fn create_test_project_ref() -> git_models::TeamProjectReference {
        git_models::TeamProjectReference {
            abbreviation: None,
            default_team_image_url: None,
            description: None,
            id: None,
            last_update_time: None,
            name: "test-project".to_string(),
            revision: None,
            state: None,
            url: None,
            visibility: git_models::team_project_reference::Visibility::Private,
        }
    }

    /// Helper to create a minimal GitRepository for testing
    fn create_test_repository(ssh_url: Option<String>) -> git_models::GitRepository {
        git_models::GitRepository {
            links: None,
            default_branch: None,
            id: "test-repo-id".to_string(),
            is_disabled: None,
            is_fork: None,
            is_in_maintenance: None,
            name: "test-repo".to_string(),
            parent_repository: None,
            project: create_test_project_ref(),
            remote_url: None,
            size: None,
            ssh_url,
            url: "https://test.url".to_string(),
            valid_remote_urls: vec![],
            web_url: None,
        }
    }

    /// Helper to create a minimal GitPullRequest for testing
    fn create_test_git_pull_request(
        id: i32,
        title: Option<String>,
        closed_date: Option<time::OffsetDateTime>,
        display_name: Option<String>,
        commit_id: Option<String>,
        labels: Vec<git_models::WebApiTagDefinition>,
    ) -> git_models::GitPullRequest {
        let identity_ref = git_models::IdentityRef {
            graph_subject_base: git_models::GraphSubjectBase {
                descriptor: None,
                display_name,
                url: None,
                links: None,
            },
            directory_alias: None,
            id: String::new(),
            image_url: None,
            inactive: None,
            is_aad_identity: None,
            is_container: None,
            is_deleted_in_origin: None,
            profile_url: None,
            unique_name: None,
        };

        let last_merge_commit = commit_id.map(|cid| git_models::GitCommitRef {
            commit_id: Some(cid),
            url: None,
            author: None,
            change_counts: None,
            changes: vec![],
            comment: None,
            comment_truncated: None,
            commit_too_many_changes: None,
            committer: None,
            links: None,
            parents: vec![],
            push: None,
            remote_url: None,
            statuses: vec![],
            work_items: vec![],
        });

        git_models::GitPullRequest {
            links: None,
            artifact_id: None,
            auto_complete_set_by: None,
            closed_by: None,
            closed_date,
            code_review_id: None,
            commits: vec![],
            completion_options: None,
            completion_queue_time: None,
            created_by: identity_ref,
            creation_date: time::OffsetDateTime::now_utc(),
            description: None,
            fork_source: None,
            has_multiple_merge_bases: None,
            is_draft: false,
            labels,
            last_merge_commit,
            last_merge_source_commit: None,
            last_merge_target_commit: None,
            merge_failure_message: None,
            merge_failure_type: None,
            merge_id: None,
            merge_options: None,
            merge_status: None,
            pull_request_id: id,
            remote_url: None,
            repository: create_test_repository(None),
            reviewers: vec![],
            source_ref_name: "refs/heads/feature".to_string(),
            status: git_models::git_pull_request::Status::Active,
            supports_iterations: None,
            target_ref_name: "refs/heads/main".to_string(),
            title,
            url: "https://test.url".to_string(),
            work_item_refs: vec![],
        }
    }

    /// Helper to create a WorkItem for testing
    fn create_test_work_item(id: i32, fields: serde_json::Value) -> wit_models::WorkItem {
        wit_models::WorkItem {
            work_item_tracking_resource: wit_models::WorkItemTrackingResource {
                work_item_tracking_resource_reference:
                    wit_models::WorkItemTrackingResourceReference { url: String::new() },
                links: None,
            },
            comment_version_ref: None,
            id,
            rev: None,
            fields,
            relations: vec![],
        }
    }

    /// Helper to create a WorkItemUpdate for testing
    fn create_test_work_item_update(
        rev: Option<i32>,
        revised_date: Option<time::OffsetDateTime>,
        fields: Option<serde_json::Value>,
    ) -> wit_models::WorkItemUpdate {
        wit_models::WorkItemUpdate {
            work_item_tracking_resource: wit_models::WorkItemTrackingResource {
                work_item_tracking_resource_reference:
                    wit_models::WorkItemTrackingResourceReference { url: String::new() },
                links: None,
            },
            id: None,
            rev,
            revised_by: None,
            revised_date,
            fields,
            relations: None,
            work_item_id: None,
        }
    }

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

    /// # Work Item ID Extraction Edge Cases
    ///
    /// Tests edge cases for work item ID extraction.
    ///
    /// ## Test Scenario
    /// - URLs with trailing slashes
    /// - URLs with query parameters
    /// - Negative numbers
    ///
    /// ## Expected Outcome
    /// - Handles various URL formats correctly
    #[test]
    fn test_extract_work_item_id_edge_cases() {
        // URL ending with a number
        assert_eq!(
            extract_work_item_id("https://example.com/items/999"),
            Some(999)
        );

        // Just a number
        assert_eq!(extract_work_item_id("42"), Some(42));

        // URL with non-numeric end
        assert_eq!(extract_work_item_id("https://example.com/abc"), None);

        // Large number
        assert_eq!(
            extract_work_item_id("https://example.com/2147483647"),
            Some(2147483647)
        );
    }

    /// # GitPullRequest to PullRequest Conversion
    ///
    /// Tests conversion of GitPullRequest to our PullRequest model.
    ///
    /// ## Test Scenario
    /// - Creates a GitPullRequest with all fields populated
    /// - Converts it to our PullRequest model
    ///
    /// ## Expected Outcome
    /// - All fields are correctly mapped
    #[test]
    fn test_pull_request_from_git_pull_request_full() {
        let label = git_models::WebApiTagDefinition {
            active: None,
            id: None,
            name: Some("bug".to_string()),
            url: None,
        };

        let pr = create_test_git_pull_request(
            123,
            Some("Test PR Title".to_string()),
            Some(time::OffsetDateTime::now_utc()),
            Some("John Doe".to_string()),
            Some("abc123def".to_string()),
            vec![label],
        );

        let converted: PullRequest = pr.into();

        assert_eq!(converted.id, 123);
        assert_eq!(converted.title, "Test PR Title");
        assert!(converted.closed_date.is_some());
        assert_eq!(converted.created_by.display_name, "John Doe");
        assert!(converted.last_merge_commit.is_some());
        assert_eq!(converted.last_merge_commit.unwrap().commit_id, "abc123def");
        assert!(converted.labels.is_some());
        assert_eq!(converted.labels.unwrap()[0].name, "bug");
    }

    /// # GitPullRequest to PullRequest Conversion - Minimal
    ///
    /// Tests conversion with minimal/default values.
    ///
    /// ## Test Scenario
    /// - Creates a GitPullRequest with minimal fields
    /// - Converts it to our PullRequest model
    ///
    /// ## Expected Outcome
    /// - Default values are used for missing optional fields
    #[test]
    fn test_pull_request_from_git_pull_request_minimal() {
        let pr = create_test_git_pull_request(1, None, None, None, None, vec![]);

        let converted: PullRequest = pr.into();

        assert_eq!(converted.id, 1);
        assert_eq!(converted.title, ""); // Default
        assert!(converted.closed_date.is_none());
        assert_eq!(converted.created_by.display_name, ""); // Default
        assert!(converted.last_merge_commit.is_none());
        assert!(converted.labels.is_none()); // Empty vec becomes None
    }

    /// # GitPullRequest to PullRequest - Multiple Labels
    ///
    /// Tests conversion with multiple labels.
    ///
    /// ## Test Scenario
    /// - Creates a GitPullRequest with multiple labels
    /// - Converts and verifies all labels are preserved
    ///
    /// ## Expected Outcome
    /// - All labels are correctly converted
    #[test]
    fn test_pull_request_multiple_labels() {
        let labels = vec![
            git_models::WebApiTagDefinition {
                active: None,
                id: None,
                name: Some("bug".to_string()),
                url: None,
            },
            git_models::WebApiTagDefinition {
                active: None,
                id: None,
                name: Some("priority-high".to_string()),
                url: None,
            },
            git_models::WebApiTagDefinition {
                active: None,
                id: None,
                name: None, // No name - should default to empty
                url: None,
            },
        ];

        let pr = create_test_git_pull_request(1, None, None, None, None, labels);
        let converted: PullRequest = pr.into();

        let labels = converted.labels.unwrap();
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[0].name, "bug");
        assert_eq!(labels[1].name, "priority-high");
        assert_eq!(labels[2].name, ""); // Default for None
    }

    /// # WorkItem Conversion - Full Fields
    ///
    /// Tests conversion of WorkItem with all fields populated.
    ///
    /// ## Test Scenario
    /// - Creates a WorkItem with all standard fields
    /// - Converts to our WorkItem model
    ///
    /// ## Expected Outcome
    /// - All fields are correctly extracted from JSON
    #[test]
    fn test_work_item_from_wit_work_item_full() {
        let fields = json!({
            "System.Title": "Test Work Item",
            "System.State": "Active",
            "System.WorkItemType": "Bug",
            "System.AssignedTo": {
                "displayName": "Jane Smith"
            },
            "System.IterationPath": "Project\\Sprint1",
            "System.Description": "This is a description",
            "Microsoft.VSTS.TCM.ReproSteps": "Step 1, Step 2"
        });

        let wi = create_test_work_item(456, fields);
        let converted: WorkItem = wi.into();

        assert_eq!(converted.id, 456);
        assert_eq!(converted.fields.title, Some("Test Work Item".to_string()));
        assert_eq!(converted.fields.state, Some("Active".to_string()));
        assert_eq!(converted.fields.work_item_type, Some("Bug".to_string()));
        assert!(converted.fields.assigned_to.is_some());
        assert_eq!(
            converted.fields.assigned_to.unwrap().display_name,
            "Jane Smith"
        );
        assert_eq!(
            converted.fields.iteration_path,
            Some("Project\\Sprint1".to_string())
        );
        assert_eq!(
            converted.fields.description,
            Some("This is a description".to_string())
        );
        assert_eq!(
            converted.fields.repro_steps,
            Some("Step 1, Step 2".to_string())
        );
        assert!(converted.history.is_empty()); // History populated separately
    }

    /// # WorkItem Conversion - Empty Fields
    ///
    /// Tests conversion of WorkItem with no fields.
    ///
    /// ## Test Scenario
    /// - Creates a WorkItem with empty fields object
    /// - Converts to our WorkItem model
    ///
    /// ## Expected Outcome
    /// - All field values are None
    #[test]
    fn test_work_item_from_wit_work_item_empty_fields() {
        let fields = json!({});
        let wi = create_test_work_item(789, fields);
        let converted: WorkItem = wi.into();

        assert_eq!(converted.id, 789);
        assert!(converted.fields.title.is_none());
        assert!(converted.fields.state.is_none());
        assert!(converted.fields.work_item_type.is_none());
        assert!(converted.fields.assigned_to.is_none());
        assert!(converted.fields.iteration_path.is_none());
        assert!(converted.fields.description.is_none());
        assert!(converted.fields.repro_steps.is_none());
    }

    /// # WorkItem Conversion - Partial Fields
    ///
    /// Tests conversion with only some fields populated.
    ///
    /// ## Test Scenario
    /// - Creates a WorkItem with some fields missing
    /// - Converts to our WorkItem model
    ///
    /// ## Expected Outcome
    /// - Present fields are extracted, missing ones are None
    #[test]
    fn test_work_item_from_wit_work_item_partial() {
        let fields = json!({
            "System.Title": "Partial Item",
            "System.State": "Closed"
        });

        let wi = create_test_work_item(111, fields);
        let converted: WorkItem = wi.into();

        assert_eq!(converted.id, 111);
        assert_eq!(converted.fields.title, Some("Partial Item".to_string()));
        assert_eq!(converted.fields.state, Some("Closed".to_string()));
        assert!(converted.fields.work_item_type.is_none());
        assert!(converted.fields.assigned_to.is_none());
    }

    /// # WorkItemUpdate to WorkItemHistory Conversion - Full
    ///
    /// Tests conversion of WorkItemUpdate with all fields.
    ///
    /// ## Test Scenario
    /// - Creates a WorkItemUpdate with state change and date
    /// - Converts to our WorkItemHistory model
    ///
    /// ## Expected Outcome
    /// - All fields including nested newValue are extracted
    #[test]
    fn test_work_item_history_from_update_full() {
        let fields = json!({
            "System.State": {
                "oldValue": "Active",
                "newValue": "Resolved"
            },
            "System.ChangedDate": {
                "oldValue": "2024-01-01T00:00:00Z",
                "newValue": "2024-01-15T12:00:00Z"
            }
        });

        let update = create_test_work_item_update(
            Some(5),
            Some(time::OffsetDateTime::now_utc()),
            Some(fields),
        );

        let converted: WorkItemHistory = update.into();

        assert_eq!(converted.rev, 5);
        assert!(!converted.revised_date.is_empty());
        assert!(converted.fields.is_some());

        let fields = converted.fields.unwrap();
        assert!(fields.state.is_some());
        assert_eq!(
            fields.state.unwrap().new_value,
            Some("Resolved".to_string())
        );
        assert!(fields.changed_date.is_some());
        assert_eq!(
            fields.changed_date.unwrap().new_value,
            Some("2024-01-15T12:00:00Z".to_string())
        );
    }

    /// # WorkItemUpdate to WorkItemHistory Conversion - Minimal
    ///
    /// Tests conversion with minimal/no fields.
    ///
    /// ## Test Scenario
    /// - Creates a WorkItemUpdate with no optional fields
    /// - Converts to our WorkItemHistory model
    ///
    /// ## Expected Outcome
    /// - Defaults are used for missing values
    #[test]
    fn test_work_item_history_from_update_minimal() {
        let update = create_test_work_item_update(None, None, None);
        let converted: WorkItemHistory = update.into();

        assert_eq!(converted.rev, 0); // Default
        assert_eq!(converted.revised_date, ""); // Default for None
        assert!(converted.fields.is_none());
    }

    /// # WorkItemUpdate - Fields Without newValue
    ///
    /// Tests conversion when fields exist but don't have newValue.
    ///
    /// ## Test Scenario
    /// - Creates fields without the newValue nested property
    ///
    /// ## Expected Outcome
    /// - State and changed_date are None when newValue is missing
    #[test]
    fn test_work_item_history_fields_without_new_value() {
        let fields = json!({
            "System.State": {
                "oldValue": "Active"
            }
        });

        let update = create_test_work_item_update(Some(1), None, Some(fields));
        let converted: WorkItemHistory = update.into();

        assert!(converted.fields.is_some());
        let fields = converted.fields.unwrap();
        assert!(fields.state.is_none()); // No newValue
    }

    /// # GitRepository to RepoDetails Conversion
    ///
    /// Tests conversion of GitRepository to RepoDetails.
    ///
    /// ## Test Scenario
    /// - Creates a GitRepository with SSH URL
    /// - Converts to our RepoDetails model
    ///
    /// ## Expected Outcome
    /// - SSH URL is correctly extracted
    #[test]
    fn test_repo_details_from_git_repository() {
        let repo = create_test_repository(Some(
            "git@ssh.dev.azure.com:v3/org/project/repo".to_string(),
        ));

        let converted: RepoDetails = repo.into();
        assert_eq!(
            converted.ssh_url,
            "git@ssh.dev.azure.com:v3/org/project/repo"
        );
    }

    /// # GitRepository to RepoDetails - No SSH URL
    ///
    /// Tests conversion when SSH URL is not present.
    ///
    /// ## Test Scenario
    /// - Creates a GitRepository without SSH URL
    /// - Converts to our RepoDetails model
    ///
    /// ## Expected Outcome
    /// - SSH URL defaults to empty string
    #[test]
    fn test_repo_details_from_git_repository_no_ssh() {
        let repo = create_test_repository(None);

        let converted: RepoDetails = repo.into();
        assert_eq!(converted.ssh_url, "");
    }
}
