//! Post-merge operations for tagging PRs and updating work items.
//!
//! This module provides the core logic for completing a merge by tagging
//! successful PRs and updating work item states in Azure DevOps.

use std::sync::Arc;

/// A task to be performed as part of post-merge completion.
#[derive(Debug, Clone)]
pub enum PostMergeTask {
    /// Tag a PR with a version label.
    TagPR {
        /// The PR ID to tag.
        pr_id: i32,
        /// The PR title (for display).
        pr_title: String,
        /// The tag to apply (e.g., "merged-v1.0.0").
        tag: String,
    },
    /// Update a work item's state.
    UpdateWorkItem {
        /// The work item ID to update.
        work_item_id: i32,
        /// The work item title (for display).
        work_item_title: String,
        /// The new state to set.
        new_state: String,
    },
}

impl PostMergeTask {
    /// Returns a human-readable description of the task.
    pub fn description(&self) -> String {
        match self {
            PostMergeTask::TagPR { pr_id, tag, .. } => {
                format!("Tag PR #{} with '{}'", pr_id, tag)
            }
            PostMergeTask::UpdateWorkItem {
                work_item_id,
                new_state,
                ..
            } => {
                format!("Update work item #{} to '{}'", work_item_id, new_state)
            }
        }
    }

    /// Returns the target ID (PR or work item ID).
    pub fn target_id(&self) -> i32 {
        match self {
            PostMergeTask::TagPR { pr_id, .. } => *pr_id,
            PostMergeTask::UpdateWorkItem { work_item_id, .. } => *work_item_id,
        }
    }
}

/// Result of executing a single post-merge task.
#[derive(Debug, Clone)]
pub enum PostMergeTaskResult {
    /// Task completed successfully.
    Success,
    /// Task failed with an error.
    Failed {
        /// Error message.
        message: String,
    },
}

impl PostMergeTaskResult {
    /// Returns true if the task succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, PostMergeTaskResult::Success)
    }

    /// Returns true if the task failed.
    pub fn is_failed(&self) -> bool {
        matches!(self, PostMergeTaskResult::Failed { .. })
    }
}

/// A task with its execution result.
#[derive(Debug, Clone)]
pub struct TaskWithResult {
    /// The task that was executed.
    pub task: PostMergeTask,
    /// The result of execution.
    pub result: Option<PostMergeTaskResult>,
}

impl TaskWithResult {
    /// Creates a new pending task.
    pub fn new(task: PostMergeTask) -> Self {
        Self { task, result: None }
    }

    /// Returns true if this task is still pending.
    pub fn is_pending(&self) -> bool {
        self.result.is_none()
    }

    /// Returns true if this task succeeded.
    pub fn is_success(&self) -> bool {
        self.result
            .as_ref()
            .map(|r| r.is_success())
            .unwrap_or(false)
    }

    /// Returns true if this task failed.
    pub fn is_failed(&self) -> bool {
        self.result.as_ref().map(|r| r.is_failed()).unwrap_or(false)
    }
}

/// Progress update for post-merge operations.
#[derive(Debug, Clone)]
pub enum PostMergeProgress {
    /// Starting post-merge tasks.
    Starting {
        /// Total number of tasks.
        total_tasks: usize,
    },
    /// Task started.
    TaskStarted {
        /// Task index.
        index: usize,
        /// Task description.
        description: String,
    },
    /// Task completed.
    TaskCompleted {
        /// Task index.
        index: usize,
        /// Task result.
        result: PostMergeTaskResult,
    },
    /// All tasks complete.
    AllComplete {
        /// Number of successful tasks.
        success_count: usize,
        /// Number of failed tasks.
        failed_count: usize,
    },
}

/// Configuration for post-merge operations.
#[derive(Debug, Clone)]
pub struct PostMergeConfig {
    /// Tag prefix (e.g., "merged-").
    pub tag_prefix: String,
    /// Version string (e.g., "v1.0.0").
    pub version: String,
    /// State to set work items to.
    pub work_item_state: String,
}

/// Result of the complete post-merge operation.
#[derive(Debug, Clone)]
pub struct PostMergeResult {
    /// All tasks with their results.
    pub tasks: Vec<TaskWithResult>,
    /// Number of successful tasks.
    pub success_count: usize,
    /// Number of failed tasks.
    pub failed_count: usize,
}

impl PostMergeResult {
    /// Returns true if all tasks succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.failed_count == 0
    }

    /// Returns failed tasks for retry.
    pub fn failed_tasks(&self) -> Vec<&TaskWithResult> {
        self.tasks.iter().filter(|t| t.is_failed()).collect()
    }
}

/// Information about a successfully cherry-picked PR.
#[derive(Debug, Clone)]
pub struct CompletedPRInfo {
    /// The PR ID.
    pub pr_id: i32,
    /// The PR title.
    pub pr_title: String,
    /// Work items associated with this PR.
    pub work_items: Vec<WorkItemInfo>,
}

/// Information about a work item.
#[derive(Debug, Clone)]
pub struct WorkItemInfo {
    /// The work item ID.
    pub id: i32,
    /// The work item title.
    pub title: String,
}

/// Core post-merge operation.
///
/// This struct encapsulates all the logic for tagging PRs and updating
/// work items after a successful merge.
pub struct PostMergeOperation {
    client: Arc<crate::api::AzureDevOpsClient>,
    config: PostMergeConfig,
}

impl PostMergeOperation {
    /// Creates a new post-merge operation.
    pub fn new(client: Arc<crate::api::AzureDevOpsClient>, config: PostMergeConfig) -> Self {
        Self { client, config }
    }

    /// Builds the task queue from completed PRs.
    ///
    /// # Arguments
    ///
    /// * `completed_prs` - List of successfully cherry-picked PRs
    ///
    /// # Returns
    ///
    /// A vector of tasks to execute.
    pub fn build_task_queue(&self, completed_prs: &[CompletedPRInfo]) -> Vec<TaskWithResult> {
        let tag = format!("{}{}", self.config.tag_prefix, self.config.version);
        let mut tasks = Vec::new();

        for pr in completed_prs {
            // Add tagging task for each PR
            tasks.push(TaskWithResult::new(PostMergeTask::TagPR {
                pr_id: pr.pr_id,
                pr_title: pr.pr_title.clone(),
                tag: tag.clone(),
            }));

            // Add work item update tasks
            for wi in &pr.work_items {
                tasks.push(TaskWithResult::new(PostMergeTask::UpdateWorkItem {
                    work_item_id: wi.id,
                    work_item_title: wi.title.clone(),
                    new_state: self.config.work_item_state.clone(),
                }));
            }
        }

        tasks
    }

    /// Executes a single task.
    ///
    /// # Arguments
    ///
    /// * `task` - The task to execute
    ///
    /// # Returns
    ///
    /// The result of the task execution.
    pub async fn execute_task(&self, task: &PostMergeTask) -> PostMergeTaskResult {
        match task {
            PostMergeTask::TagPR { pr_id, tag, .. } => {
                match self.client.add_label_to_pr(*pr_id, tag).await {
                    Ok(_) => PostMergeTaskResult::Success,
                    Err(e) => PostMergeTaskResult::Failed {
                        message: e.to_string(),
                    },
                }
            }
            PostMergeTask::UpdateWorkItem {
                work_item_id,
                new_state,
                ..
            } => match self
                .client
                .update_work_item_state(*work_item_id, new_state)
                .await
            {
                Ok(_) => PostMergeTaskResult::Success,
                Err(e) => PostMergeTaskResult::Failed {
                    message: e.to_string(),
                },
            },
        }
    }

    /// Executes all tasks in the queue.
    ///
    /// # Arguments
    ///
    /// * `tasks` - Mutable slice of tasks to execute
    /// * `progress_callback` - Optional callback for progress updates
    ///
    /// # Returns
    ///
    /// A `PostMergeResult` with all task results.
    pub async fn execute_all<F>(
        &self,
        tasks: &mut [TaskWithResult],
        mut progress_callback: Option<F>,
    ) -> PostMergeResult
    where
        F: FnMut(PostMergeProgress),
    {
        if let Some(ref mut callback) = progress_callback {
            callback(PostMergeProgress::Starting {
                total_tasks: tasks.len(),
            });
        }

        let mut success_count = 0;
        let mut failed_count = 0;

        for (idx, task_item) in tasks.iter_mut().enumerate() {
            // Skip already-completed tasks
            if task_item.result.is_some() {
                if task_item.is_success() {
                    success_count += 1;
                } else {
                    failed_count += 1;
                }
                continue;
            }

            if let Some(ref mut callback) = progress_callback {
                callback(PostMergeProgress::TaskStarted {
                    index: idx,
                    description: task_item.task.description(),
                });
            }

            let result = self.execute_task(&task_item.task).await;

            if result.is_success() {
                success_count += 1;
            } else {
                failed_count += 1;
            }

            if let Some(ref mut callback) = progress_callback {
                callback(PostMergeProgress::TaskCompleted {
                    index: idx,
                    result: result.clone(),
                });
            }

            task_item.result = Some(result);
        }

        if let Some(ref mut callback) = progress_callback {
            callback(PostMergeProgress::AllComplete {
                success_count,
                failed_count,
            });
        }

        PostMergeResult {
            tasks: tasks.to_vec(),
            success_count,
            failed_count,
        }
    }

    /// Retries failed tasks.
    ///
    /// # Arguments
    ///
    /// * `tasks` - Mutable slice of tasks (failed ones will be retried)
    /// * `progress_callback` - Optional callback for progress updates
    ///
    /// # Returns
    ///
    /// A `PostMergeResult` with updated results.
    pub async fn retry_failed<F>(
        &self,
        tasks: &mut [TaskWithResult],
        progress_callback: Option<F>,
    ) -> PostMergeResult
    where
        F: FnMut(PostMergeProgress),
    {
        // Reset failed tasks to pending
        for task in tasks.iter_mut() {
            if task.is_failed() {
                task.result = None;
            }
        }

        // Re-execute
        self.execute_all(tasks, progress_callback).await
    }
}

/// Extracts work items info from PRs for post-merge tasks.
pub fn extract_completed_pr_info(
    prs: &[crate::models::PullRequestWithWorkItems],
    successful_pr_ids: &[i32],
) -> Vec<CompletedPRInfo> {
    prs.iter()
        .filter(|pr| successful_pr_ids.contains(&pr.pr.id))
        .map(|pr| CompletedPRInfo {
            pr_id: pr.pr.id,
            pr_title: pr.pr.title.clone(),
            work_items: pr
                .work_items
                .iter()
                .map(|wi| WorkItemInfo {
                    id: wi.id,
                    title: wi.fields.title.clone().unwrap_or_default(),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Post Merge Task Description
    ///
    /// Verifies that task descriptions are human-readable.
    ///
    /// ## Test Scenario
    /// - Creates different task types and checks descriptions
    ///
    /// ## Expected Outcome
    /// - Descriptions contain relevant information
    #[test]
    fn test_post_merge_task_description() {
        let tag_task = PostMergeTask::TagPR {
            pr_id: 42,
            pr_title: "Test PR".to_string(),
            tag: "merged-v1.0.0".to_string(),
        };
        let desc = tag_task.description();
        assert!(desc.contains("42"));
        assert!(desc.contains("merged-v1.0.0"));

        let update_task = PostMergeTask::UpdateWorkItem {
            work_item_id: 123,
            work_item_title: "Test WI".to_string(),
            new_state: "Done".to_string(),
        };
        let desc = update_task.description();
        assert!(desc.contains("123"));
        assert!(desc.contains("Done"));
    }

    /// # Post Merge Task Target ID
    ///
    /// Verifies that target_id returns the correct ID.
    ///
    /// ## Test Scenario
    /// - Creates different task types and checks target IDs
    ///
    /// ## Expected Outcome
    /// - Returns PR ID for TagPR, work item ID for UpdateWorkItem
    #[test]
    fn test_post_merge_task_target_id() {
        let tag_task = PostMergeTask::TagPR {
            pr_id: 42,
            pr_title: "Test".to_string(),
            tag: "merged".to_string(),
        };
        assert_eq!(tag_task.target_id(), 42);

        let update_task = PostMergeTask::UpdateWorkItem {
            work_item_id: 123,
            work_item_title: "Test".to_string(),
            new_state: "Done".to_string(),
        };
        assert_eq!(update_task.target_id(), 123);
    }

    /// # Task With Result States
    ///
    /// Verifies TaskWithResult state queries.
    ///
    /// ## Test Scenario
    /// - Creates tasks in different states
    ///
    /// ## Expected Outcome
    /// - State queries return correct values
    #[test]
    fn test_task_with_result_states() {
        let pending = TaskWithResult::new(PostMergeTask::TagPR {
            pr_id: 1,
            pr_title: "Test".to_string(),
            tag: "merged".to_string(),
        });
        assert!(pending.is_pending());
        assert!(!pending.is_success());
        assert!(!pending.is_failed());

        let success = TaskWithResult {
            task: PostMergeTask::TagPR {
                pr_id: 1,
                pr_title: "Test".to_string(),
                tag: "merged".to_string(),
            },
            result: Some(PostMergeTaskResult::Success),
        };
        assert!(!success.is_pending());
        assert!(success.is_success());
        assert!(!success.is_failed());

        let failed = TaskWithResult {
            task: PostMergeTask::TagPR {
                pr_id: 1,
                pr_title: "Test".to_string(),
                tag: "merged".to_string(),
            },
            result: Some(PostMergeTaskResult::Failed {
                message: "error".to_string(),
            }),
        };
        assert!(!failed.is_pending());
        assert!(!failed.is_success());
        assert!(failed.is_failed());
    }

    /// # Post Merge Result All Succeeded
    ///
    /// Verifies all_succeeded logic.
    ///
    /// ## Test Scenario
    /// - Creates results with different success/failure counts
    ///
    /// ## Expected Outcome
    /// - all_succeeded returns true only when failed_count is 0
    #[test]
    fn test_post_merge_result_all_succeeded() {
        let success = PostMergeResult {
            tasks: Vec::new(),
            success_count: 5,
            failed_count: 0,
        };
        assert!(success.all_succeeded());

        let partial = PostMergeResult {
            tasks: Vec::new(),
            success_count: 3,
            failed_count: 2,
        };
        assert!(!partial.all_succeeded());
    }

    /// # Post Merge Progress Variants
    ///
    /// Verifies that all progress variants can be created.
    ///
    /// ## Test Scenario
    /// - Creates each progress variant
    ///
    /// ## Expected Outcome
    /// - All variants construct successfully
    #[test]
    fn test_post_merge_progress_variants() {
        let _p1 = PostMergeProgress::Starting { total_tasks: 10 };
        let _p2 = PostMergeProgress::TaskStarted {
            index: 0,
            description: "Test".to_string(),
        };
        let _p3 = PostMergeProgress::TaskCompleted {
            index: 0,
            result: PostMergeTaskResult::Success,
        };
        let _p4 = PostMergeProgress::AllComplete {
            success_count: 8,
            failed_count: 2,
        };
    }

    /// # Extract Completed PR Info
    ///
    /// Verifies extraction of PR info for post-merge tasks.
    ///
    /// ## Test Scenario
    /// - Creates PRs with work items, extracts info for successful ones
    ///
    /// ## Expected Outcome
    /// - Only successful PRs are included with their work items
    #[test]
    fn test_extract_completed_pr_info() {
        use crate::models::{
            CreatedBy, PullRequest, PullRequestWithWorkItems, WorkItem, WorkItemFields,
        };

        let prs = vec![
            PullRequestWithWorkItems {
                pr: PullRequest {
                    id: 1,
                    title: "PR 1".to_string(),
                    closed_date: None,
                    created_by: CreatedBy {
                        display_name: "user".to_string(),
                    },
                    labels: None,
                    last_merge_commit: None,
                },
                work_items: vec![WorkItem {
                    id: 101,
                    fields: WorkItemFields {
                        title: Some("WI 101".to_string()),
                        state: Some("Active".to_string()),
                        work_item_type: None,
                        assigned_to: None,
                        iteration_path: None,
                        description: None,
                        repro_steps: None,
                        state_color: None,
                    },
                    history: Vec::new(),
                }],
                selected: false,
            },
            PullRequestWithWorkItems {
                pr: PullRequest {
                    id: 2,
                    title: "PR 2".to_string(),
                    closed_date: None,
                    created_by: CreatedBy {
                        display_name: "user".to_string(),
                    },
                    labels: None,
                    last_merge_commit: None,
                },
                work_items: vec![],
                selected: false,
            },
        ];

        let result = extract_completed_pr_info(&prs, &[1]);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pr_id, 1);
        assert_eq!(result[0].work_items.len(), 1);
        assert_eq!(result[0].work_items[0].id, 101);
    }
}
