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
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
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
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(f.area());

        let title = Paragraph::new("🏁 Cherry-pick Process Completed!")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Split the main area horizontally
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(main_chunks[1]);

        // Left side: Commit status list
        let items: Vec<ListItem> = app
            .cherry_pick_items
            .iter()
            .map(|item| {
                let mut spans = vec![];

                let (symbol, color) = match &item.status {
                    CherryPickStatus::Success => ("✅", Color::Green),
                    CherryPickStatus::Failed(_) => ("❌", Color::Red),
                    CherryPickStatus::Skipped => ("⏭️", Color::Gray),
                    CherryPickStatus::Conflict => ("⚠️", Color::Yellow),
                    _ => ("❓", Color::White),
                };

                spans.push(Span::styled(
                    format!("{} ", symbol),
                    Style::default().fg(color),
                ));
                spans.push(Span::styled(
                    format!("PR #{}: ", item.pr_id),
                    Style::default().fg(Color::Cyan),
                ));

                // Truncate title if needed
                let title = if item.pr_title.len() > 40 {
                    format!("{}...", &item.pr_title[..37])
                } else {
                    item.pr_title.clone()
                };
                spans.push(Span::raw(title));

                if let CherryPickStatus::Failed(msg) = &item.status {
                    spans.push(Span::styled(
                        format!(" - {}", if msg.len() > 30 { &msg[..27] } else { msg }),
                        Style::default().fg(Color::Red),
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Cherry-pick Results"));
        f.render_widget(list, content_chunks[0]);

        // Right side: Summary and info
        let mut summary_text = vec![];

        // Calculate summary
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

        summary_text.push(Line::from(vec![
            Span::styled("Summary", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(""));
        summary_text.push(Line::from(vec![
            Span::raw("✅ Successful: "),
            Span::styled(format!("{}", successful), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(vec![
            Span::raw("❌ Failed: "),
            Span::styled(format!("{}", failed), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(vec![
            Span::raw("⏭️  Skipped: "),
            Span::styled(format!("{}", skipped), Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD)),
        ]));
        
        summary_text.push(Line::from(""));
        summary_text.push(Line::from("─────────────────────"));
        summary_text.push(Line::from(""));
        
        summary_text.push(Line::from(vec![
            Span::styled("Branch Info", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(""));
        
        let branch_name = format!(
            "patch/{}-{}",
            app.target_branch,
            app.version.as_ref().unwrap()
        );
        summary_text.push(Line::from(vec![
            Span::raw("Branch: "),
            Span::styled(branch_name, Style::default().fg(Color::Cyan)),
        ]));
        
        if let Some(repo_path) = &app.repo_path {
            summary_text.push(Line::from(vec![
                Span::raw("Location: "),
                Span::styled(
                    format!("{}", repo_path.display()),
                    Style::default().fg(Color::Blue),
                ),
            ]));
        }
        
        summary_text.push(Line::from(""));
        summary_text.push(Line::from("─────────────────────"));
        summary_text.push(Line::from(""));
        
        summary_text.push(Line::from(vec![
            Span::styled("Actions", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(""));
        summary_text.push(Line::from("Press 'q' to exit"));

        let summary = Paragraph::new(summary_text)
            .block(Block::default().borders(Borders::ALL).title("Summary & Info"))
            .wrap(Wrap { trim: true });
        f.render_widget(summary, content_chunks[1]);
    }

    async fn process_key(&mut self, code: KeyCode, _app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }
}