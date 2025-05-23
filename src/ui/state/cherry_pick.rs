use crate::{
    git,
    models::CherryPickStatus,
    ui::App,
    ui::state::{AppState, CompletionState, ConflictResolutionState, ErrorState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub struct CherryPickState {
    processing: bool,
}

impl CherryPickState {
    pub fn new() -> Self {
        Self { processing: true }
    }
}

#[async_trait]
impl AppState for CherryPickState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area());

        let title = Paragraph::new("Cherry-picking Commits")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Split the main area horizontally
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);

        // Left side: Commit list
        let items: Vec<ListItem> = app
            .cherry_pick_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let mut spans = vec![];

                let (symbol, color) = match &item.status {
                    CherryPickStatus::Pending => ("⏸", Color::Gray),
                    CherryPickStatus::InProgress => ("⏳", Color::Yellow),
                    CherryPickStatus::Success => ("✅", Color::Green),
                    CherryPickStatus::Conflict => ("⚠️", Color::Yellow),
                    CherryPickStatus::Failed(_) => ("❌", Color::Red),
                    CherryPickStatus::Skipped => ("⏭️", Color::Gray),
                };

                spans.push(Span::styled(
                    format!("{} ", symbol),
                    Style::default().fg(color),
                ));
                spans.push(Span::raw(format!(
                    "[{}/{}] ",
                    i + 1,
                    app.cherry_pick_items.len()
                )));
                spans.push(Span::styled(
                    format!("PR #{}: ", item.pr_id),
                    Style::default().fg(Color::Cyan),
                ));
                
                // Truncate title if too long
                let title = if item.pr_title.len() > 40 {
                    format!("{}...", &item.pr_title[..37])
                } else {
                    item.pr_title.clone()
                };
                spans.push(Span::raw(title));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Commits"))
            .highlight_style(Style::default().bg(Color::DarkGray));
        f.render_widget(list, main_chunks[0]);

        // Right side: Details
        let mut details_text = vec![];
        
        if app.current_cherry_pick_index < app.cherry_pick_items.len() {
            let current_item = &app.cherry_pick_items[app.current_cherry_pick_index];
            
            details_text.push(Line::from(vec![
                Span::raw("Current PR: "),
                Span::styled(
                    format!("#{}", current_item.pr_id),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]));
            
            details_text.push(Line::from(""));
            details_text.push(Line::from(vec![
                Span::raw("Title: "),
                Span::raw(&current_item.pr_title),
            ]));
            
            details_text.push(Line::from(""));
            details_text.push(Line::from(vec![
                Span::raw("Commit: "),
                Span::styled(
                    &current_item.commit_id[..8],
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            
            details_text.push(Line::from(""));
            details_text.push(Line::from(vec![
                Span::raw("Status: "),
                Span::styled(
                    match &current_item.status {
                        CherryPickStatus::Pending => "Pending",
                        CherryPickStatus::InProgress => "In Progress",
                        CherryPickStatus::Success => "Success",
                        CherryPickStatus::Conflict => "Conflict",
                        CherryPickStatus::Failed(_) => "Failed",
                        CherryPickStatus::Skipped => "Skipped",
                    },
                    Style::default().fg(match &current_item.status {
                        CherryPickStatus::Success => Color::Green,
                        CherryPickStatus::Failed(_) => Color::Red,
                        CherryPickStatus::Conflict => Color::Yellow,
                        CherryPickStatus::InProgress => Color::Yellow,
                        CherryPickStatus::Skipped => Color::Gray,
                        _ => Color::White,
                    }),
                ),
            ]));
            
            if let CherryPickStatus::Failed(msg) = &current_item.status {
                details_text.push(Line::from(""));
                details_text.push(Line::from(vec![
                    Span::raw("Error: "),
                    Span::styled(msg, Style::default().fg(Color::Red)),
                ]));
            }
        }
        
        details_text.push(Line::from(""));
        details_text.push(Line::from("─────────────────────"));
        details_text.push(Line::from(""));
        
        let branch_name = format!(
            "patch/{}-{}",
            app.target_branch,
            app.version.as_ref().unwrap()
        );
        
        details_text.push(Line::from(vec![
            Span::raw("Branch: "),
            Span::styled(branch_name, Style::default().fg(Color::Cyan)),
        ]));
        
        if let Some(repo_path) = &app.repo_path {
            details_text.push(Line::from(vec![
                Span::raw("Location: "),
                Span::styled(
                    format!("{}", repo_path.display()),
                    Style::default().fg(Color::Blue),
                ),
            ]));
        }

        let details = Paragraph::new(details_text)
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .wrap(Wrap { trim: true });
        f.render_widget(details, main_chunks[1]);

        let status = if self.processing {
            "Processing cherry-picks..."
        } else {
            "Press any key to continue"
        };
        let status_widget = Paragraph::new(status)
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(status_widget, chunks[2]);
    }

    async fn process_key(&mut self, _code: KeyCode, app: &mut App) -> StateChange {
        if !self.processing {
            return StateChange::Keep;
        }

        self.processing = false;

        let repo_path = app.repo_path.as_ref().unwrap();
        let version = app.version.as_ref().unwrap();
        let branch_name = format!("patch/{}-{}", app.target_branch, version);

        // Create branch
        if let Err(e) = git::create_branch(repo_path, &branch_name) {
            app.error_message = Some(format!("Failed to create branch: {}", e));
            return StateChange::Change(Box::new(ErrorState::new()));
        }

        // Fetch commits if needed
        if app.local_repo.is_none() {
            let commits: Vec<String> = app
                .cherry_pick_items
                .iter()
                .map(|item| item.commit_id.clone())
                .collect();

            if let Err(e) = git::fetch_commits(repo_path, &commits) {
                app.error_message = Some(format!("Failed to fetch commits: {}", e));
                return StateChange::Change(Box::new(ErrorState::new()));
            }
        }

        // Process first commit
        process_next_commit(app)
    }
}

pub fn process_next_commit(app: &mut App) -> StateChange {
    while app.current_cherry_pick_index < app.cherry_pick_items.len() {
        let item = &mut app.cherry_pick_items[app.current_cherry_pick_index];

        if !matches!(item.status, CherryPickStatus::Pending) {
            app.current_cherry_pick_index += 1;
            continue;
        }

        item.status = CherryPickStatus::InProgress;
        let commit_id = item.commit_id.clone();
        let repo_path = app.repo_path.as_ref().unwrap();

        match git::cherry_pick_commit(repo_path, &commit_id) {
            Ok(git::CherryPickResult::Success) => {
                item.status = CherryPickStatus::Success;
                app.current_cherry_pick_index += 1;
            }
            Ok(git::CherryPickResult::Conflict(files)) => {
                item.status = CherryPickStatus::Conflict;
                return StateChange::Change(Box::new(ConflictResolutionState::new(files)));
            }
            Ok(git::CherryPickResult::Failed(msg)) => {
                item.status = CherryPickStatus::Failed(msg);
                app.current_cherry_pick_index += 1;
            }
            Err(e) => {
                item.status = CherryPickStatus::Failed(e.to_string());
                app.current_cherry_pick_index += 1;
            }
        }
    }

    StateChange::Change(Box::new(CompletionState::new()))
}