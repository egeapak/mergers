use crate::{
    git,
    models::CherryPickItem,
    ui::App,
    ui::state::{AppState, CherryPickState, ErrorState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[derive(Debug, Clone)]
pub enum SetupState {
    Initializing,
    InProgress(String),
    Error {
        error: git::RepositorySetupError,
        message: String,
    },
}

pub struct SetupRepoState {
    state: SetupState,
    started: bool,
}

impl SetupRepoState {
    pub fn new() -> Self {
        Self {
            state: SetupState::Initializing,
            started: false,
        }
    }

    fn set_status(&mut self, status: String) {
        self.state = SetupState::InProgress(status);
    }

    fn set_error(&mut self, error: git::RepositorySetupError) {
        let message = match &error {
            git::RepositorySetupError::BranchExists(branch) => {
                format!(
                    "Branch '{}' already exists.\n\nThis can happen if you've run this tool before or if the branch was created elsewhere.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force delete the branch and continue\n  • Press 'Esc' to go back",
                    branch
                )
            }
            git::RepositorySetupError::WorktreeExists(path) => {
                format!(
                    "Worktree already exists at:\n{}\n\nThis can happen if you've run this tool before or if the worktree was created elsewhere.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force remove the worktree and continue\n  • Press 'Esc' to go back",
                    path
                )
            }
            git::RepositorySetupError::Other(msg) => {
                format!(
                    "Setup failed: {}\n\nOptions:\n  • Press 'r' to retry\n  • Press 'Esc' to go back",
                    msg
                )
            }
        };
        self.state = SetupState::Error {
            error: error.clone(),
            message,
        };
    }

    async fn setup_repository(&mut self, app: &mut App) -> StateChange {
        // Get SSH URL if needed
        let ssh_url = if app.local_repo.is_none() {
            self.set_status("Fetching repository details...".to_string());
            match app.client.fetch_repo_details().await {
                Ok(details) => details.ssh_url,
                Err(e) => {
                    app.error_message = Some(format!("Failed to fetch repository details: {}", e));
                    return StateChange::Change(Box::new(ErrorState::new()));
                }
            }
        } else {
            String::new()
        };

        let version = app.version.as_ref().unwrap();

        self.set_status(if app.local_repo.is_some() {
            "Creating worktree...".to_string()
        } else {
            "Cloning repository...".to_string()
        });

        // Setup repository
        match git::setup_repository(
            app.local_repo.as_deref(),
            &ssh_url,
            &app.target_branch,
            version,
        ) {
            Ok(setup) => {
                match setup {
                    git::RepositorySetup::Local(path) => {
                        app.repo_path = Some(path);
                    }
                    git::RepositorySetup::Clone(path, temp_dir) => {
                        app.repo_path = Some(path);
                        app._temp_dir = Some(temp_dir);
                    }
                }

                // Prepare cherry-pick items
                let selected_prs = app.get_selected_prs();
                let mut cherry_pick_items = Vec::new();

                for pr in selected_prs {
                    if let Some(commit) = &pr.pr.last_merge_commit {
                        cherry_pick_items.push(CherryPickItem {
                            commit_id: commit.commit_id.clone(),
                            pr_id: pr.pr.id,
                            pr_title: pr.pr.title.clone(),
                            status: crate::models::CherryPickStatus::Pending,
                        });
                    }
                }

                if cherry_pick_items.is_empty() {
                    app.error_message = Some("No commits found to cherry-pick".to_string());
                    StateChange::Change(Box::new(ErrorState::new()))
                } else {
                    app.cherry_pick_items = cherry_pick_items;
                    
                    // Create branch for cherry-picking
                    self.set_status("Creating branch...".to_string());
                    let branch_name = format!("patch/{}-{}", app.target_branch, version);
                    
                    if let Err(e) = git::create_branch(app.repo_path.as_ref().unwrap(), &branch_name) {
                        app.error_message = Some(format!("Failed to create branch: {}", e));
                        StateChange::Change(Box::new(ErrorState::new()))
                    } else {
                        StateChange::Change(Box::new(CherryPickState::new()))
                    }
                }
            }
            Err(e) => {
                self.set_error(e);
                StateChange::Keep
            }
        }
    }

    async fn force_resolve_error(
        &mut self,
        app: &mut App,
        error: git::RepositorySetupError,
    ) -> StateChange {
        let version = app.version.as_ref().unwrap();

        match error {
            git::RepositorySetupError::BranchExists(branch_name) => {
                self.set_status("Force deleting branch...".to_string());
                if let Some(repo_path) = &app.local_repo {
                    if let Err(e) =
                        git::force_delete_branch(std::path::Path::new(repo_path), &branch_name)
                    {
                        app.error_message = Some(format!("Failed to force delete branch: {}", e));
                        return StateChange::Change(Box::new(ErrorState::new()));
                    }
                }
            }
            git::RepositorySetupError::WorktreeExists(_) => {
                self.set_status("Force removing worktree...".to_string());
                if let Some(repo_path) = &app.local_repo {
                    if let Err(e) =
                        git::force_remove_worktree(std::path::Path::new(repo_path), version)
                    {
                        app.error_message = Some(format!("Failed to force remove worktree: {}", e));
                        return StateChange::Change(Box::new(ErrorState::new()));
                    }
                }
            }
            git::RepositorySetupError::Other(_) => {
                // For other errors, just retry
            }
        }

        // After force operation, retry the setup
        self.setup_repository(app).await
    }
}

#[async_trait]
impl AppState for SetupRepoState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Min(0)])
            .split(f.area());

        match &self.state {
            SetupState::Initializing => {
                let status = Paragraph::new("Initializing repository...")
                    .style(Style::default().fg(Color::Yellow))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Repository Setup"),
                    )
                    .alignment(Alignment::Center);

                f.render_widget(status, chunks[0]);
            }
            SetupState::InProgress(message) => {
                let status = Paragraph::new(message.as_str())
                    .style(Style::default().fg(Color::Yellow))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Repository Setup"),
                    )
                    .alignment(Alignment::Center);

                f.render_widget(status, chunks[0]);
            }
            SetupState::Error { message, .. } => {
                let message_lines: Vec<Line> = message
                    .lines()
                    .map(|line| {
                        if line.starts_with("Options:") {
                            Line::from(vec![Span::styled(line, Style::default().fg(Color::Cyan))])
                        } else if line.starts_with("  •") {
                            Line::from(vec![Span::styled(line, Style::default().fg(Color::Yellow))])
                        } else {
                            Line::from(line)
                        }
                    })
                    .collect();

                let error_paragraph = Paragraph::new(message_lines)
                    .style(Style::default().fg(Color::White))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Repository Setup Error")
                            .title_style(Style::default().fg(Color::Red)),
                    )
                    .wrap(Wrap { trim: true })
                    .alignment(Alignment::Left);

                f.render_widget(error_paragraph, chunks[0]);
            }
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        match &self.state {
            SetupState::Error { error, .. } => {
                match code {
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        // Retry - reset state and try again
                        self.state = SetupState::Initializing;
                        self.started = false;
                        self.setup_repository(app).await
                    }
                    KeyCode::Char('f') | KeyCode::Char('F') => {
                        // Force - try to resolve the specific error and retry
                        let error_clone = error.clone();
                        self.force_resolve_error(app, error_clone).await
                    }
                    KeyCode::Esc => {
                        // Go back to previous state or exit
                        StateChange::Change(Box::new(ErrorState::new()))
                    }
                    _ => StateChange::Keep,
                }
            }
            _ => {
                if !self.started {
                    self.started = true;
                    self.setup_repository(app).await
                } else {
                    StateChange::Keep
                }
            }
        }
    }
}
