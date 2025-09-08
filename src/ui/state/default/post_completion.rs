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

        let _version = app.version.as_ref().unwrap();

        // Add tasks for tagging successful PRs
        for item in &app.cherry_pick_items {
            if matches!(item.status, CherryPickStatus::Success) {
                self.tasks.push(PostCompletionTaskItem {
                    task: PostCompletionTask::TaggingPR {
                        pr_id: item.pr_id,
                        pr_title: item.pr_title.clone(),
                    },
                    status: TaskStatus::Pending,
                });

                // Add tasks for updating work items associated with successful PRs
                if let Some(pr_data) = app.pull_requests.iter().find(|pr| pr.pr.id == item.pr_id) {
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
                let version = app.version.as_ref().unwrap();
                let tag_name = format!("{}{}", app.tag_prefix, version);
                app.client.add_label_to_pr(*pr_id, &tag_name).await
            }
            PostCompletionTask::UpdatingWorkItem { work_item_id, .. } => {
                app.client
                    .update_work_item_state(*work_item_id, &app.work_item_state)
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
                        work_item_id, app.work_item_state, work_item_title
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
                    app.tag_prefix,
                    app.version.as_ref().unwrap()
                )),
                Line::from(format!(
                    "✅ Work items updated to '{}'",
                    app.work_item_state
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
                    app.tag_prefix,
                    app.version.as_ref().unwrap()
                )),
                Line::from(format!(
                    "📝 Updating work items to '{}'",
                    app.work_item_state
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
