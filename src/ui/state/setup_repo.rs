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
    widgets::{Block, Borders, Paragraph},
};

pub struct SetupRepoState {
    status: String,
    in_progress: bool,
}

impl SetupRepoState {
    pub fn new() -> Self {
        Self {
            status: "Initializing repository...".to_string(),
            in_progress: true,
        }
    }
}

#[async_trait]
impl AppState for SetupRepoState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Min(0)])
            .split(f.size());

        let status = Paragraph::new(self.status.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Repository Setup"),
            )
            .alignment(Alignment::Center);

        f.render_widget(status, chunks[0]);
    }

    async fn process_key(&mut self, _code: KeyCode, app: &mut App) -> StateChange {
        if !self.in_progress {
            return StateChange::Keep;
        }

        self.in_progress = false;

        // Get SSH URL if needed
        let ssh_url = if app.local_repo.is_none() {
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

        // Setup repository
        match git::setup_repository(
            app.local_repo.as_deref(),
            &ssh_url,
            &app.target_branch,
            version,
        ) {
            Ok(repo_path) => {
                app.repo_path = Some(repo_path);

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
                    return StateChange::Change(Box::new(ErrorState::new()));
                }

                app.cherry_pick_items = cherry_pick_items;
                StateChange::Change(Box::new(CherryPickState::new()))
            }
            Err(e) => {
                app.error_message = Some(format!("Failed to setup repository: {}", e));
                StateChange::Change(Box::new(ErrorState::new()))
            }
        }
    }
}
