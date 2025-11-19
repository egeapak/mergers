use crate::{
    git::force_delete_branch,
    models::CleanupStatus,
    ui::App,
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

    fn start_cleanup(&mut self, app: &mut App) {
        if self.deletion_tasks.is_some() {
            return;
        }

        self.start_time = Some(Instant::now());

        // Get repo path
        let repo_path = match &app.repo_path {
            Some(path) => path.clone(),
            None => {
                // This shouldn't happen, but handle it gracefully
                for branch in &mut app.cleanup_branches {
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
        for (idx, branch) in app.cleanup_branches.iter_mut().enumerate() {
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

    async fn check_progress(&mut self, app: &mut App) -> bool {
        if let Some(tasks) = &mut self.deletion_tasks {
            let mut all_complete = true;

            for task in tasks.iter_mut() {
                if !task.is_finished() {
                    all_complete = false;
                    continue;
                }

                // Process completed task
                if let Ok((idx, result)) = task.await
                    && idx < app.cleanup_branches.len()
                {
                    app.cleanup_branches[idx].status = match result {
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

    fn get_progress(&self, app: &App) -> (usize, usize) {
        let total = app.cleanup_branches.iter().filter(|b| b.selected).count();
        let completed = app
            .cleanup_branches
            .iter()
            .filter(|b| {
                b.selected && matches!(b.status, CleanupStatus::Success | CleanupStatus::Failed(_))
            })
            .count();
        (completed, total)
    }
}

#[async_trait]
impl AppState for CleanupExecutionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
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
            .cleanup_branches
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

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Enter if self.is_complete => {
                StateChange::Change(Box::new(CleanupResultsState::new()))
            }
            KeyCode::Null => {
                // Poll for task completion
                if !self.is_complete {
                    if self.deletion_tasks.is_none() {
                        self.start_cleanup(app);
                    }

                    if self.check_progress(app).await {
                        // Auto-transition to results after a brief moment
                        // (in real impl, we might want to add a small delay here)
                        return StateChange::Change(Box::new(CleanupResultsState::new()));
                    }
                }
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }
}
