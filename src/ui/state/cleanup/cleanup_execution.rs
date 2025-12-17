use super::CleanupModeState;
use crate::{
    git::force_delete_branch,
    models::CleanupStatus,
    ui::App,
    ui::apps::CleanupApp,
    ui::state::typed::{TypedAppState, TypedStateChange},
    ui::state::{AppState, CleanupResultsState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
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
        if let Some(tasks) = &mut self.deletion_tasks {
            let mut all_complete = true;

            for task in tasks.iter_mut() {
                if !task.is_finished() {
                    all_complete = false;
                    continue;
                }

                // Process completed task
                if let Ok((idx, result)) = task.await
                    && idx < app.cleanup_branches().len()
                {
                    app.cleanup_branches_mut()[idx].status = match result {
                        Ok(_) => CleanupStatus::Success,
                        Err(e) => CleanupStatus::Failed(e),
                    };
                }
            }

            if all_complete {
                self.is_complete = true;
                return true;
            }
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
// TypedAppState Implementation
// ============================================================================

#[async_trait]
impl TypedAppState for CleanupExecutionState {
    type App = CleanupApp;
    type StateEnum = CleanupModeState;

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
        let help_text = if self.is_complete {
            "Cleanup complete. Press Enter to view results, or 'q' to exit"
        } else {
            "Deleting branches... Please wait"
        };

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(help, chunks[3]);
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut CleanupApp,
    ) -> TypedStateChange<CleanupModeState> {
        match code {
            KeyCode::Char('q') => TypedStateChange::Exit,
            KeyCode::Enter if self.is_complete => {
                TypedStateChange::Change(CleanupModeState::Results(CleanupResultsState::new()))
            }
            KeyCode::Null => {
                // Poll for task completion
                if !self.is_complete {
                    if self.deletion_tasks.is_none() {
                        self.start_cleanup(app);
                    }

                    if self.check_progress(app).await {
                        // Auto-transition to results after a brief moment
                        return TypedStateChange::Change(CleanupModeState::Results(
                            CleanupResultsState::new(),
                        ));
                    }
                }
                TypedStateChange::Keep
            }
            _ => TypedStateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "CleanupExecution"
    }
}

// ============================================================================
// Legacy AppState Implementation
// ============================================================================

#[async_trait]
impl AppState for CleanupExecutionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        if let App::Cleanup(cleanup_app) = app {
            TypedAppState::ui(self, f, cleanup_app);
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        if let App::Cleanup(cleanup_app) = app {
            match <Self as TypedAppState>::process_key(self, code, cleanup_app).await {
                TypedStateChange::Keep => StateChange::Keep,
                TypedStateChange::Exit => StateChange::Exit,
                TypedStateChange::Change(new_state) => StateChange::Change(Box::new(new_state)),
            }
        } else {
            StateChange::Keep
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::CleanupBranch, models::CleanupStatus, ui::testing::*};
    use insta::assert_snapshot;

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

            let state = Box::new(CleanupExecutionState::new());
            harness.render_state(state);
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

            let state = Box::new(CleanupExecutionState::new());
            harness.render_state(state);
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

            harness.render_state(Box::new(state));
            assert_snapshot!("complete", harness.backend());
        });
    }
}
