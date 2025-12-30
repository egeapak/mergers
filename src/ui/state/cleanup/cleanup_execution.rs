use super::CleanupModeState;
use crate::{
    git::force_delete_branch,
    models::CleanupStatus,
    ui::apps::CleanupApp,
    ui::state::CleanupResultsState,
    ui::state::typed::{ModeState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
};
use std::time::Instant;

type DeletionTask = tokio::task::JoinHandle<(usize, Result<(), String>)>;

pub struct CleanupExecutionState {
    is_complete: bool,
    start_time: Option<Instant>,
    deletion_tasks: Option<Vec<DeletionTask>>,
}

impl Default for CleanupExecutionState {
    fn default() -> Self {
        Self::new()
    }
}

impl CleanupExecutionState {
    pub fn new() -> Self {
        Self {
            is_complete: false,
            start_time: None,
            deletion_tasks: None,
        }
    }

    fn start_cleanup(&mut self, app: &mut CleanupApp) {
        if self.deletion_tasks.is_some() {
            return;
        }

        self.start_time = Some(Instant::now());

        // Get repo path (clone it to avoid borrow issues)
        let repo_path_opt = app.repo_path().map(|p| p.to_path_buf());

        let repo_path = match repo_path_opt {
            Some(path) => path,
            None => {
                // This shouldn't happen, but handle it gracefully
                for branch in app.cleanup_branches_mut() {
                    if branch.selected {
                        branch.status =
                            CleanupStatus::Failed("No repository path available".to_string());
                    }
                }
                self.is_complete = true;
                return;
            }
        };

        // Spawn deletion tasks for selected branches
        let mut tasks = Vec::new();
        for (idx, branch) in app.cleanup_branches_mut().iter_mut().enumerate() {
            if branch.selected {
                branch.status = CleanupStatus::InProgress;
                let branch_name = branch.name.clone();
                let repo_path_clone = repo_path.clone();

                let task = tokio::spawn(async move {
                    let result = force_delete_branch(&repo_path_clone, &branch_name)
                        .map_err(|e| e.to_string());
                    (idx, result)
                });

                tasks.push(task);
            }
        }

        self.deletion_tasks = Some(tasks);
    }

    async fn check_progress(&mut self, app: &mut CleanupApp) -> bool {
        // Take ownership of tasks to avoid polling completed JoinHandles
        if let Some(tasks) = self.deletion_tasks.take() {
            let mut pending_tasks = Vec::new();

            for task in tasks {
                if !task.is_finished() {
                    // Task still running - keep it for next poll
                    pending_tasks.push(task);
                    continue;
                }

                // Process completed task (consumes the JoinHandle)
                if let Ok((idx, result)) = task.await
                    && idx < app.cleanup_branches().len()
                {
                    app.cleanup_branches_mut()[idx].status = match result {
                        Ok(_) => CleanupStatus::Success,
                        Err(e) => CleanupStatus::Failed(e),
                    };
                }
            }

            if pending_tasks.is_empty() {
                self.is_complete = true;
                return true;
            }

            // Put remaining tasks back
            self.deletion_tasks = Some(pending_tasks);
        }

        false
    }

    fn get_progress(&self, app: &CleanupApp) -> (usize, usize) {
        let total = app.cleanup_branches().iter().filter(|b| b.selected).count();
        let completed = app
            .cleanup_branches()
            .iter()
            .filter(|b| {
                b.selected && matches!(b.status, CleanupStatus::Success | CleanupStatus::Failed(_))
            })
            .count();
        (completed, total)
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for CleanupExecutionState {
    type Mode = CleanupModeState;

    fn ui(&mut self, f: &mut Frame, app: &CleanupApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("Cleanup Mode - Deleting Branches")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Progress bar
        let (completed, total) = self.get_progress(app);
        let progress_percent = if total > 0 {
            ((completed as f64 / total as f64) * 100.0) as u16
        } else {
            0
        };

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .percent(progress_percent)
            .label(format!("Deleted {}/{} branches", completed, total));
        f.render_widget(gauge, chunks[1]);

        // Branch status list
        let items: Vec<ListItem> = app
            .cleanup_branches()
            .iter()
            .filter(|b| b.selected)
            .map(|branch| {
                let (symbol, color) = match &branch.status {
                    CleanupStatus::Pending => ("⏳", Color::Gray),
                    CleanupStatus::InProgress => ("🔄", Color::Yellow),
                    CleanupStatus::Success => ("✅", Color::Green),
                    CleanupStatus::Failed(_) => ("❌", Color::Red),
                };

                let status_text = match &branch.status {
                    CleanupStatus::Pending => "Pending",
                    CleanupStatus::InProgress => "Deleting...",
                    CleanupStatus::Success => "Deleted",
                    CleanupStatus::Failed(e) => e.as_str(),
                };

                let content = format!("{} {} - {}", symbol, branch.name, status_text);
                ListItem::new(content).style(Style::default().fg(color))
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(list, chunks[2]);

        // Help text
        let help_lines = if self.is_complete {
            vec![Line::from(vec![
                Span::raw("Cleanup complete. Press "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to view results, or "),
                Span::styled(
                    "q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to exit"),
            ])]
        } else {
            vec![Line::from("Deleting branches... Please wait")]
        };

        let help = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(help, chunks[3]);
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut CleanupApp,
    ) -> StateChange<CleanupModeState> {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Enter if self.is_complete => {
                StateChange::Change(CleanupModeState::Results(CleanupResultsState::new()))
            }
            KeyCode::Null => {
                // Poll for task completion
                if !self.is_complete {
                    if self.deletion_tasks.is_none() {
                        self.start_cleanup(app);
                    }

                    if self.check_progress(app).await {
                        // Auto-transition to results after a brief moment
                        return StateChange::Change(CleanupModeState::Results(
                            CleanupResultsState::new(),
                        ));
                    }
                }
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "CleanupExecution"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::CleanupBranch, models::CleanupStatus, ui::testing::*};
    use insta::assert_snapshot;

    /// # Check Progress Multiple Polls Test
    ///
    /// Tests that check_progress can be called multiple times without panic.
    ///
    /// ## Test Scenario
    /// - Creates a CleanupExecutionState with pre-completed tasks
    /// - Calls check_progress multiple times
    /// - This simulates KeyCode::Null events arriving repeatedly
    ///
    /// ## Expected Outcome
    /// - Should NOT panic with "JoinHandle polled after completion"
    /// - Should correctly mark completion after all tasks finish
    /// - Subsequent calls should be safe (no-op when no tasks remain)
    ///
    /// ## Regression Test
    /// This test verifies the fix for the panic that occurred when
    /// iter_mut() was used instead of take() - completed JoinHandles
    /// remained in the vector and panicked on subsequent polls.
    #[tokio::test]
    async fn test_check_progress_multiple_polls_no_panic() {
        let config = create_test_config_cleanup();
        let mut harness = TuiTestHarness::with_config(config);

        // Set up branches
        *harness.app.cleanup_branches_mut() = vec![
            CleanupBranch {
                name: "branch-1".to_string(),
                target: "main".to_string(),
                version: "1.0.0".to_string(),
                is_merged: true,
                selected: true,
                status: CleanupStatus::InProgress,
            },
            CleanupBranch {
                name: "branch-2".to_string(),
                target: "main".to_string(),
                version: "1.0.1".to_string(),
                is_merged: true,
                selected: true,
                status: CleanupStatus::InProgress,
            },
        ];

        let mut state = CleanupExecutionState::new();

        // Manually set up tasks that complete immediately
        let tasks: Vec<DeletionTask> = vec![
            tokio::spawn(async { (0, Ok(())) }),
            tokio::spawn(async { (1, Ok(())) }),
        ];
        state.deletion_tasks = Some(tasks);

        // Wait briefly for tasks to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // First call - should process completed tasks
        let completed = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(completed, "Should report completion after all tasks finish");
        assert!(state.is_complete, "State should be marked complete");

        // Second call - should NOT panic (this is the regression test)
        // With the old iter_mut() code, this would panic with:
        // "JoinHandle polled after completion"
        let completed_again = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(
            !completed_again,
            "Should return false when no tasks to process"
        );

        // Third call - still should not panic
        let completed_third = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(
            !completed_third,
            "Should return false when no tasks to process"
        );
    }

    /// # Check Progress With Pending Tasks Test
    ///
    /// Tests that check_progress correctly handles a mix of completed
    /// and still-running tasks across multiple calls.
    ///
    /// ## Test Scenario
    /// - Creates tasks where some complete quickly and others take longer
    /// - Calls check_progress multiple times
    /// - Verifies pending tasks are preserved for next poll
    ///
    /// ## Expected Outcome
    /// - Completed tasks should be processed and removed
    /// - Pending tasks should remain for subsequent polls
    /// - No panic should occur
    #[tokio::test]
    async fn test_check_progress_preserves_pending_tasks() {
        let config = create_test_config_cleanup();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cleanup_branches_mut() = vec![
            CleanupBranch {
                name: "fast-branch".to_string(),
                target: "main".to_string(),
                version: "1.0.0".to_string(),
                is_merged: true,
                selected: true,
                status: CleanupStatus::InProgress,
            },
            CleanupBranch {
                name: "slow-branch".to_string(),
                target: "main".to_string(),
                version: "1.0.1".to_string(),
                is_merged: true,
                selected: true,
                status: CleanupStatus::InProgress,
            },
        ];

        let mut state = CleanupExecutionState::new();

        // Create tasks: one completes immediately, one takes longer
        let tasks: Vec<DeletionTask> = vec![
            tokio::spawn(async { (0, Ok(())) }), // Completes immediately
            tokio::spawn(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                (1, Ok(()))
            }), // Takes longer
        ];
        state.deletion_tasks = Some(tasks);

        // Wait for first task to complete but not the second
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

        // First call - should process completed task, keep pending one
        let completed = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(
            !completed,
            "Should not be complete - one task still pending"
        );
        assert!(
            state.deletion_tasks.is_some(),
            "Should still have pending tasks"
        );

        // Verify the fast branch was updated
        assert!(matches!(
            harness.app.cleanup_branches()[0].status,
            CleanupStatus::Success
        ));

        // Second call - slow task still running, should not panic
        let completed = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(!completed, "Should still not be complete");

        // Wait for slow task to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Third call - now should complete
        let completed = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(completed, "Should be complete now");

        // Fourth call - should not panic
        let completed = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(!completed, "Should return false, no more tasks");
    }

    /// # Check Progress With Failed Tasks Test
    ///
    /// Tests that check_progress correctly handles task failures
    /// and can still be called multiple times without panic.
    ///
    /// ## Test Scenario
    /// - Creates tasks where some succeed and some fail
    /// - Calls check_progress multiple times
    ///
    /// ## Expected Outcome
    /// - Failed tasks should update branch status to Failed
    /// - No panic should occur on subsequent calls
    #[tokio::test]
    async fn test_check_progress_with_failed_tasks() {
        let config = create_test_config_cleanup();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cleanup_branches_mut() = vec![
            CleanupBranch {
                name: "success-branch".to_string(),
                target: "main".to_string(),
                version: "1.0.0".to_string(),
                is_merged: true,
                selected: true,
                status: CleanupStatus::InProgress,
            },
            CleanupBranch {
                name: "fail-branch".to_string(),
                target: "main".to_string(),
                version: "1.0.1".to_string(),
                is_merged: true,
                selected: true,
                status: CleanupStatus::InProgress,
            },
        ];

        let mut state = CleanupExecutionState::new();

        let tasks: Vec<DeletionTask> = vec![
            tokio::spawn(async { (0, Ok(())) }),
            tokio::spawn(async { (1, Err("Branch is protected".to_string())) }),
        ];
        state.deletion_tasks = Some(tasks);

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // First call
        let completed = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(completed);

        // Verify statuses
        assert!(matches!(
            harness.app.cleanup_branches()[0].status,
            CleanupStatus::Success
        ));
        assert!(matches!(
            &harness.app.cleanup_branches()[1].status,
            CleanupStatus::Failed(msg) if msg == "Branch is protected"
        ));

        // Second call - should not panic
        let completed = state.check_progress(harness.cleanup_app_mut()).await;
        assert!(!completed);
    }

    /// # Cleanup Execution Initial State Test
    ///
    /// Tests the cleanup execution screen at start.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds branches ready for deletion
    /// - Renders the initial execution screen
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Deleting Branches" title
    /// - Should show progress bar at 0%
    /// - Should list all selected branches with pending status
    /// - Should display "Deleting branches... Please wait" message
    #[test]
    fn test_cleanup_execution_initial() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add branches for cleanup
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Pending,
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Pending,
                },
            ];

            let state = CleanupExecutionState::new();
            harness.render_cleanup_state(&mut CleanupModeState::Execution(state));
            assert_snapshot!("initial", harness.backend());
        });
    }

    /// # Cleanup Execution In Progress Test
    ///
    /// Tests the cleanup execution screen during deletion.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds branches with mixed statuses (in progress, success, pending)
    /// - Renders the execution screen
    ///
    /// ## Expected Outcome
    /// - Should display progress bar showing partial completion
    /// - Should show different status indicators (⏳, 🔄, ✅)
    /// - Should display "Deleted X/N branches" in progress bar
    /// - Should show color-coded status for each branch
    #[test]
    fn test_cleanup_execution_in_progress() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add branches with mixed statuses
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.3".to_string(),
                    target: "main".to_string(),
                    version: "6.6.3".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::InProgress,
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Pending,
                },
            ];

            let state = CleanupExecutionState::new();
            harness.render_cleanup_state(&mut CleanupModeState::Execution(state));
            assert_snapshot!("in_progress", harness.backend());
        });
    }

    /// # Cleanup Execution Complete Test
    ///
    /// Tests the cleanup execution screen when all deletions are complete.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds branches with completed statuses (success and failed)
    /// - Marks the execution as complete
    /// - Renders the execution screen
    ///
    /// ## Expected Outcome
    /// - Should display progress bar at 100%
    /// - Should show all branches with final statuses
    /// - Should display "Cleanup complete. Press Enter to view results, or 'q' to exit"
    /// - Should show ✅ for successful deletions
    /// - Should show ❌ for failed deletions
    #[test]
    fn test_cleanup_execution_complete() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add branches with final statuses
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.3".to_string(),
                    target: "main".to_string(),
                    version: "6.6.3".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Failed("Branch is checked out".to_string()),
                },
            ];

            let mut state = CleanupExecutionState::new();
            state.is_complete = true;

            harness.render_cleanup_state(&mut CleanupModeState::Execution(state));
            assert_snapshot!("complete", harness.backend());
        });
    }
}
