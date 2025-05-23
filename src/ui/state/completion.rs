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
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub struct CompletionState;

impl CompletionState {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AppState for CompletionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(5),
            ])
            .split(f.area());

        let title = Paragraph::new("🏁 Cherry-pick Process Completed!")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Summary
        let mut successful = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for item in &app.cherry_pick_items {
            match &item.status {
                CherryPickStatus::Success => successful += 1,
                CherryPickStatus::Failed(_) => failed += 1,
                CherryPickStatus::Skipped => skipped += 1,
                _ => {}
            }
        }

        let summary_items = vec![
            ListItem::new(Line::from(vec![
                Span::raw("✅ Successful: "),
                Span::styled(format!("{}", successful), Style::default().fg(Color::Green)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("❌ Failed: "),
                Span::styled(format!("{}", failed), Style::default().fg(Color::Red)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("⏭️  Skipped: "),
                Span::styled(format!("{}", skipped), Style::default().fg(Color::Gray)),
            ])),
        ];

        let summary =
            List::new(summary_items).block(Block::default().borders(Borders::ALL).title("Summary"));
        f.render_widget(summary, chunks[1]);

        let branch_name = format!(
            "patch/{}-{}",
            app.target_branch,
            app.version.as_ref().unwrap()
        );
        let repo_path = app.repo_path.as_ref().unwrap().display();

        let info = vec![
            Line::from(vec![
                Span::raw("Branch: "),
                Span::styled(branch_name, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::raw("Location: "),
                Span::styled(format!("{}", repo_path), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from("Press 'q' to exit"),
        ];

        let info_widget = Paragraph::new(info).block(Block::default().borders(Borders::ALL));
        f.render_widget(info_widget, chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, _app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }
}
