use crate::{
    git,
    models::CherryPickStatus,
    ui::App,
    ui::state::{AppState, StateChange, CherryPickState},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub struct ConflictResolutionState {
    conflicted_files: Vec<String>,
}

impl ConflictResolutionState {
    pub fn new(conflicted_files: Vec<String>) -> Self {
        Self { conflicted_files }
    }
}

#[async_trait]
impl AppState for ConflictResolutionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(5),
                Constraint::Min(0),
                Constraint::Length(5),
                Constraint::Length(3),
            ])
            .split(f.area());

        let title = Paragraph::new("⚠️  Merge Conflict Detected")
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        let files: Vec<ListItem> = self
            .conflicted_files
            .iter()
            .map(|file| ListItem::new(format!("  • {}", file)))
            .collect();

        let file_list = List::new(files)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Conflicted Files"),
            )
            .style(Style::default().fg(Color::Red));
        f.render_widget(file_list, chunks[1]);

        let repo_path = app.repo_path.as_ref().unwrap().display();
        let instructions = vec![
            Line::from(vec![
                Span::raw("Repository: "),
                Span::styled(format!("{}", repo_path), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from("Please resolve conflicts in another terminal and stage the changes."),
        ];

        let instructions_widget = Paragraph::new(instructions)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .style(Style::default().fg(Color::White));
        f.render_widget(instructions_widget, chunks[2]);

        let help = Paragraph::new("c: Continue (after resolving) | a: Abort")
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(help, chunks[3]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        let repo_path = app.repo_path.as_ref().unwrap();

        match code {
            KeyCode::Char('c') => {
                // Check if conflicts are resolved
                match git::check_conflicts_resolved(repo_path) {
                    Ok(true) => {
                        // Continue cherry-pick
                        match git::continue_cherry_pick(repo_path) {
                            Ok(_) => {
                                app.cherry_pick_items[app.current_cherry_pick_index].status =
                                    CherryPickStatus::Success;
                                app.current_cherry_pick_index += 1;
                                StateChange::Change(Box::new(CherryPickState::new()))
                            }
                            Err(e) => {
                                app.cherry_pick_items[app.current_cherry_pick_index].status =
                                    CherryPickStatus::Failed(e.to_string());
                                app.current_cherry_pick_index += 1;
                                StateChange::Change(Box::new(CherryPickState::new()))
                            }
                        }
                    }
                    Ok(false) => StateChange::Keep, // Conflicts not resolved
                    Err(_) => StateChange::Keep,
                }
            }
            KeyCode::Char('a') => {
                // Abort entire process
                let _ = git::abort_cherry_pick(repo_path);
                StateChange::Change(Box::new(super::CompletionState::new()))
            }
            _ => StateChange::Keep,
        }
    }
}
