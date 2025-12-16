use crate::{
    models::CherryPickStatus,
    ui::App,
    ui::state::{AppState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};

#[derive(Debug, Clone)]
pub enum PostCompletionTask {
    TaggingPR {
        pr_id: i32,
        pr_title: String,
    },
    UpdatingWorkItem {
        work_item_id: i32,
        work_item_title: String,
    },
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Success,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct PostCompletionTaskItem {
    pub task: PostCompletionTask,
    pub status: TaskStatus,
}

pub struct PostCompletionState {
    tasks: Vec<PostCompletionTaskItem>,
    current_task_index: usize,
    completed: bool,
    total_tasks: usize,
}

impl Default for PostCompletionState {
    fn default() -> Self {
        Self::new()
    }
}

impl PostCompletionState {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            current_task_index: 0,
            completed: false,
            total_tasks: 0,
        }
    }

    fn retry_failed_tasks(&mut self) {
        for task_item in &mut self.tasks {
            if matches!(task_item.status, TaskStatus::Failed(_)) {
                task_item.status = TaskStatus::Pending;
            }
        }

        // Reset current task index to the first pending task
        self.current_task_index = self
            .tasks
            .iter()
            .position(|task| matches!(task.status, TaskStatus::Pending))
            .unwrap_or(self.tasks.len());

        self.completed = false;
    }

    fn has_failed_tasks(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| matches!(task.status, TaskStatus::Failed(_)))
    }

    fn initialize_tasks(&mut self, app: &App) {
        if !self.tasks.is_empty() {
            return; // Already initialized
        }

        let _version = app.version().as_ref().unwrap();

        // Add tasks for tagging successful PRs
        for item in app.cherry_pick_items() {
            if matches!(item.status, CherryPickStatus::Success) {
                self.tasks.push(PostCompletionTaskItem {
                    task: PostCompletionTask::TaggingPR {
                        pr_id: item.pr_id,
                        pr_title: item.pr_title.clone(),
                    },
                    status: TaskStatus::Pending,
                });

                // Add tasks for updating work items associated with successful PRs
                if let Some(pr_data) = app.pull_requests().iter().find(|pr| pr.pr.id == item.pr_id)
                {
                    for work_item in &pr_data.work_items {
                        if let Some(title) = &work_item.fields.title {
                            self.tasks.push(PostCompletionTaskItem {
                                task: PostCompletionTask::UpdatingWorkItem {
                                    work_item_id: work_item.id,
                                    work_item_title: title.clone(),
                                },
                                status: TaskStatus::Pending,
                            });
                        }
                    }
                }
            }
        }

        self.total_tasks = self.tasks.len();
    }

    async fn process_current_task(&mut self, app: &App) -> bool {
        if self.current_task_index >= self.tasks.len() {
            self.completed = true;
            return true;
        }

        let task_item = &mut self.tasks[self.current_task_index];

        if !matches!(task_item.status, TaskStatus::Pending) {
            self.current_task_index += 1;
            return false;
        }

        task_item.status = TaskStatus::InProgress;

        let result = match &task_item.task {
            PostCompletionTask::TaggingPR { pr_id, .. } => {
                let version = app.version().unwrap();
                let tag_name = format!("{}{}", app.tag_prefix(), version);
                app.client().add_label_to_pr(*pr_id, &tag_name).await
            }
            PostCompletionTask::UpdatingWorkItem { work_item_id, .. } => {
                app.client()
                    .update_work_item_state(*work_item_id, app.work_item_state())
                    .await
            }
        };

        match result {
            Ok(()) => {
                task_item.status = TaskStatus::Success;
            }
            Err(e) => {
                task_item.status = TaskStatus::Failed(e.to_string());
            }
        }

        self.current_task_index += 1;
        false
    }
}

#[async_trait]
impl AppState for PostCompletionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        self.initialize_tasks(app);

        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(5),
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("🏷️  Post-Completion Processing")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Progress bar
        let progress = if self.total_tasks > 0 {
            (self.current_task_index as f64 / self.total_tasks as f64 * 100.0) as u16
        } else {
            100
        };

        let progress_label = if self.completed {
            "✅ All tasks completed!".to_string()
        } else if self.total_tasks > 0 {
            format!(
                "Processing task {} of {}",
                self.current_task_index.min(self.total_tasks),
                self.total_tasks
            )
        } else {
            "No tasks to process".to_string()
        };

        let progress_gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(Style::default().fg(Color::Green))
            .percent(progress)
            .label(progress_label);
        f.render_widget(progress_gauge, main_chunks[1]);

        // Task list
        let mut task_lines = Vec::new();

        for (i, task_item) in self.tasks.iter().enumerate() {
            let (symbol, color) = match &task_item.status {
                TaskStatus::Pending => ("⏳", Color::Gray),
                TaskStatus::InProgress => ("⚡", Color::Yellow),
                TaskStatus::Success => ("✅", Color::Green),
                TaskStatus::Failed(_) => ("❌", Color::Red),
            };

            let task_description = match &task_item.task {
                PostCompletionTask::TaggingPR { pr_id, pr_title } => {
                    format!("Tag PR #{}: {}", pr_id, pr_title)
                }
                PostCompletionTask::UpdatingWorkItem {
                    work_item_id,
                    work_item_title,
                } => {
                    format!(
                        "Update WI #{} to '{}': {}",
                        work_item_id,
                        app.work_item_state(),
                        work_item_title
                    )
                }
            };

            let mut spans = vec![
                Span::styled(format!("{} ", symbol), Style::default().fg(color)),
                Span::raw(task_description),
            ];

            if let TaskStatus::Failed(error) = &task_item.status {
                spans.push(Span::styled(
                    format!(" - Error: {}", error),
                    Style::default().fg(Color::Red),
                ));
            }

            // Highlight current task
            let line_style = if i == self.current_task_index && !self.completed {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };

            task_lines.push(Line::from(spans).style(line_style));
        }

        if task_lines.is_empty() {
            task_lines.push(Line::from("No tasks to process."));
        }

        let task_list = Paragraph::new(task_lines)
            .block(Block::default().borders(Borders::ALL).title("Tasks"))
            .wrap(Wrap { trim: true });
        f.render_widget(task_list, main_chunks[2]);

        // Instructions
        let instructions = if self.completed {
            let mut lines = vec![
                Line::from("🎉 All post-completion tasks have been processed!"),
                Line::from(""),
                Line::from(format!(
                    "✅ PRs tagged with '{}{}' ",
                    app.tag_prefix(),
                    app.version().as_ref().unwrap()
                )),
                Line::from(format!(
                    "✅ Work items updated to '{}'",
                    app.work_item_state()
                )),
                Line::from(""),
            ];

            if self.has_failed_tasks() {
                lines.extend(vec![
                    Line::from("Press 'Enter' to return to completion summary"),
                    Line::from("Press 'r' to retry failed tasks"),
                    Line::from("Press 'q' to exit"),
                ]);
            } else {
                lines.extend(vec![
                    Line::from("Press 'Enter' to return to completion summary"),
                    Line::from("Press 'q' to exit"),
                ]);
            }

            lines
        } else {
            vec![
                Line::from("Processing tasks automatically..."),
                Line::from(""),
                Line::from(format!(
                    "🏷️  Tagging PRs with '{}{}' ",
                    app.tag_prefix(),
                    app.version().as_ref().unwrap()
                )),
                Line::from(format!(
                    "📝 Updating work items to '{}'",
                    app.work_item_state()
                )),
                Line::from(""),
                Line::from("Press 'q' to exit (tasks will continue in background)"),
            ]
        };

        let instructions_widget = Paragraph::new(instructions)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .wrap(Wrap { trim: true });
        f.render_widget(instructions_widget, main_chunks[3]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Null if !self.completed => {
                // Auto-process tasks
                if self.process_current_task(app).await {
                    // All tasks completed, stay in this state to show results
                }
                StateChange::Keep
            }
            KeyCode::Enter if self.completed => {
                // Return to completion state
                StateChange::Change(Box::new(crate::ui::state::CompletionState::new()))
            }
            KeyCode::Char('r') if self.completed && self.has_failed_tasks() => {
                // Retry failed tasks
                self.retry_failed_tasks();
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{TuiTestHarness, create_test_config_default, create_test_pull_requests},
    };
    use insta::assert_snapshot;

    /// # Post Completion State - Display
    ///
    /// Tests the post-completion menu screen.
    ///
    /// ## Test Scenario
    /// - Creates a post-completion state
    /// - Sets up PRs for work item updating
    /// - Renders the post-completion menu
    ///
    /// ## Expected Outcome
    /// - Should display menu options
    /// - Should show task list
    /// - Should display action instructions
    #[test]
    fn test_post_completion_display() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut prs = create_test_pull_requests();
            prs[0].selected = true;
            *harness.app.pull_requests_mut() = prs;
            harness.app.set_version(Some("v1.0.0".to_string()));

            let state = Box::new(PostCompletionState::new());
            harness.render_state(state);

            assert_snapshot!("display", harness.backend());
        });
    }

    /// # Post Completion State - Partially Updated
    ///
    /// Tests the post-completion screen with tasks in various states of completion.
    ///
    /// ## Test Scenario
    /// - Creates a post-completion state with 7 tasks
    /// - Some tasks completed successfully
    /// - One task currently in progress
    /// - Some tasks still pending
    /// - Shows progress at 3/7 tasks (43%)
    ///
    /// ## Expected Outcome
    /// - Should display progress bar showing partial completion
    /// - Should show mix of success, in-progress, and pending tasks
    /// - Should indicate which task is currently being processed
    #[test]
    fn test_post_completion_partially_updated() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut prs = create_test_pull_requests();
            prs[0].selected = true;
            *harness.app.pull_requests_mut() = prs;
            harness.app.set_version(Some("v1.0.0".to_string()));

            let tasks = crate::ui::testing::create_test_post_completion_tasks();
            let mut state = PostCompletionState::new();
            state.tasks = tasks;
            state.current_task_index = 3;
            state.total_tasks = 7;
            state.completed = false;

            harness.render_state(Box::new(state));

            assert_snapshot!("partially_updated", harness.backend());
        });
    }

    /// # Post Completion State - Permission Errors
    ///
    /// Tests the post-completion screen with multiple permission-related errors.
    ///
    /// ## Test Scenario
    /// - Creates a post-completion state
    /// - Multiple tasks failed with "403 Forbidden" permission errors
    /// - Some tasks succeeded
    /// - Marks completion as done with errors
    ///
    /// ## Expected Outcome
    /// - Should display failed tasks with permission error messages
    /// - Should show successful tasks alongside failures
    /// - Should display retry option for failed tasks
    /// - Should indicate completion with errors
    #[test]
    fn test_post_completion_with_permission_errors() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut prs = create_test_pull_requests();
            prs[0].selected = true;
            *harness.app.pull_requests_mut() = prs;
            harness.app.set_version(Some("v1.0.0".to_string()));

            let mut tasks = crate::ui::testing::create_test_post_completion_tasks();
            // Mark some tasks as successful and some with permission errors
            tasks[0].status = TaskStatus::Success;
            tasks[1].status = TaskStatus::Failed(
                "403 Forbidden: Insufficient permissions to update work item".to_string(),
            );
            tasks[2].status = TaskStatus::Success;
            tasks[3].status = TaskStatus::Failed(
                "403 Forbidden: User does not have write access to this resource".to_string(),
            );
            tasks[4].status = TaskStatus::Success;
            tasks[5].status =
                TaskStatus::Failed("403 Forbidden: Access denied for tag creation".to_string());
            tasks[6].status = TaskStatus::Success;

            let mut state = PostCompletionState::new();
            state.tasks = tasks;
            state.current_task_index = 7;
            state.total_tasks = 7;
            state.completed = true;

            harness.render_state(Box::new(state));

            assert_snapshot!("permission_errors", harness.backend());
        });
    }

    /// # Post Completion State - Mixed Errors
    ///
    /// Tests the post-completion screen with various types of errors.
    ///
    /// ## Test Scenario
    /// - Creates a post-completion state with completed tasks
    /// - Tasks failed with different error types: network, permission, timeout
    /// - Shows error details for each failure type
    ///
    /// ## Expected Outcome
    /// - Should display different error messages appropriately
    /// - Should show varied error formatting
    /// - Should provide retry option
    #[test]
    fn test_post_completion_mixed_errors() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut prs = create_test_pull_requests();
            prs[0].selected = true;
            *harness.app.pull_requests_mut() = prs;
            harness.app.set_version(Some("v1.0.0".to_string()));

            let mut tasks = crate::ui::testing::create_test_post_completion_tasks();
            tasks[0].status = TaskStatus::Success;
            tasks[1].status = TaskStatus::Failed(
                "Network error: Connection timed out after 30 seconds".to_string(),
            );
            tasks[2].status =
                TaskStatus::Failed("403 Forbidden: Insufficient permissions".to_string());
            tasks[3].status = TaskStatus::Success;
            tasks[4].status = TaskStatus::Failed(
                "API Error: Rate limit exceeded, retry after 60 seconds".to_string(),
            );
            tasks[5].status =
                TaskStatus::Failed("Invalid request: Work item ID not found".to_string());
            tasks[6].status = TaskStatus::Success;

            let mut state = PostCompletionState::new();
            state.tasks = tasks;
            state.current_task_index = 7;
            state.total_tasks = 7;
            state.completed = true;

            harness.render_state(Box::new(state));

            assert_snapshot!("mixed_errors", harness.backend());
        });
    }

    /// # Post Completion State - All Failed
    ///
    /// Tests the post-completion screen when all tasks have failed.
    ///
    /// ## Test Scenario
    /// - Creates a post-completion state
    /// - All tasks failed with various errors
    /// - Shows 0% successful completion
    ///
    /// ## Expected Outcome
    /// - Should display all tasks as failed
    /// - Should show prominent retry instruction
    /// - Should indicate failure state clearly
    #[test]
    fn test_post_completion_all_failed() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut prs = create_test_pull_requests();
            prs[0].selected = true;
            *harness.app.pull_requests_mut() = prs;
            harness.app.set_version(Some("v1.0.0".to_string()));

            let mut tasks = crate::ui::testing::create_test_post_completion_tasks();
            for task in &mut tasks {
                task.status = TaskStatus::Failed(
                    "Service unavailable: Unable to connect to Azure DevOps".to_string(),
                );
            }

            let mut state = PostCompletionState::new();
            state.tasks = tasks;
            state.current_task_index = 7;
            state.total_tasks = 7;
            state.completed = true;

            harness.render_state(Box::new(state));

            assert_snapshot!("all_failed", harness.backend());
        });
    }

    /// # Post Completion State - Quit Key
    ///
    /// Tests 'q' key to exit.
    ///
    /// ## Test Scenario
    /// - Sets completed to true
    /// - Processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_post_completion_quit() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = PostCompletionState::new();
        state.completed = true;

        let result = state
            .process_key(KeyCode::Char('q'), &mut harness.app)
            .await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Post Completion State - Other Keys
    ///
    /// Tests other keys are ignored.
    ///
    /// ## Test Scenario
    /// - Processes various unrecognized keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_post_completion_other_keys() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = PostCompletionState::new();

        for key in [KeyCode::Up, KeyCode::Down, KeyCode::Enter, KeyCode::Esc] {
            let result = state.process_key(key, &mut harness.app).await;
            assert!(matches!(result, StateChange::Keep));
        }
    }

    /// # PostCompletionState Default Implementation
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates PostCompletionState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should match PostCompletionState::new()
    #[test]
    fn test_post_completion_default() {
        let state = PostCompletionState::default();
        assert!(!state.completed);
        assert!(state.tasks.is_empty());
    }
}
