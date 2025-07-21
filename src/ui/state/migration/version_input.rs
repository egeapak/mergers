use super::MigrationResultsState;
use crate::{
    ui::App,
    ui::state::{AppState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub struct MigrationVersionInputState {
    input: String,
}

impl MigrationVersionInputState {
    pub fn new() -> Self {
        Self {
            input: String::new(),
        }
    }
}

#[async_trait]
impl AppState for MigrationVersionInputState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),  // Title
                Constraint::Length(1),  // Spacing
                Constraint::Length(3),  // Input box
                Constraint::Length(1),  // Spacing
                Constraint::Length(4),  // Summary info
                Constraint::Min(8),     // PRs NOT to be marked list
                Constraint::Length(6),  // Help text
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("Migration Mode - Version Input")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Input box
        let input_block = Paragraph::new(self.input.as_str())
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Version Number")
                    .border_style(Style::default().fg(Color::Yellow))
            );
        f.render_widget(input_block, chunks[2]);

        // Summary info
        let (eligible_count, not_marked_prs) = if let Some(analysis) = &app.migration_analysis {
            let eligible_count = analysis.eligible_prs.len();
            let mut not_marked = Vec::new();
            
            // Collect unsure and not merged PRs
            not_marked.extend(analysis.unsure_prs.clone());
            not_marked.extend(analysis.not_merged_prs.clone());
            
            (eligible_count, not_marked)
        } else {
            (0, Vec::new())
        };

        let summary_lines = vec![
            Line::from(vec![
                Span::styled("PRs to be tagged: ", Style::default().fg(Color::White)),
                Span::styled(format!("{}", eligible_count), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("PRs NOT to be tagged: ", Style::default().fg(Color::White)),
                Span::styled(format!("{}", not_marked_prs.len()), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(" (listed below)", Style::default().fg(Color::Gray))
            ]),
            Line::from(vec![
                Span::styled("Version format: ", Style::default().fg(Color::Gray)),
                Span::styled("2.1.0, 2024.1, release-candidate-1", Style::default().fg(Color::Green))
            ]),
        ];

        let summary = Paragraph::new(summary_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Summary")
                    .border_style(Style::default().fg(Color::Blue))
            );
        f.render_widget(summary, chunks[4]);

        // PRs NOT to be marked list
        let not_marked_items: Vec<Line> = if not_marked_prs.is_empty() {
            vec![
                Line::from(vec![
                    Span::styled("🎉 All PRs will be tagged!", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                ]),
                Line::from(""),
                Line::from("No PRs are excluded from tagging."),
            ]
        } else {
            let mut lines = Vec::new();
            for pr in &not_marked_prs {
                // Check if this PR has a manual override
                let override_indicator = match app.has_manual_override(pr.pr.id) {
                    Some(true) => " [📌 Manual Override]",
                    Some(false) => " [❌ Manual Override]", 
                    None => "",
                };
                
                lines.push(Line::from(vec![
                    Span::styled(format!("#{}", pr.pr.id), Style::default().fg(Color::Cyan)),
                    Span::raw(" "),
                    Span::styled(&pr.pr.title, Style::default().fg(Color::White)),
                    Span::styled(override_indicator, Style::default().fg(Color::Magenta)),
                ]));
            }
            lines
        };

        let not_marked_list = Paragraph::new(not_marked_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("PRs NOT to be Tagged")
                    .border_style(Style::default().fg(Color::Red))
            )
            .wrap(Wrap { trim: false })
            .scroll((0, 0));
        f.render_widget(not_marked_list, chunks[5]);

        // Help text
        let help_lines = vec![
            Line::from(vec![
                Span::styled("Instructions:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            ]),
            Line::from("  • Type your version number and press Enter to continue"),
            Line::from("  • Use Esc to go back to PR results"),
            Line::from("  • Use Backspace to edit your input"),
        ];

        let help = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: true });
        f.render_widget(help, chunks[6]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        match code {
            KeyCode::Char(c) => {
                self.input.push(c);
                StateChange::Keep
            }
            KeyCode::Backspace => {
                self.input.pop();
                StateChange::Keep
            }
            KeyCode::Enter => {
                if !self.input.trim().is_empty() {
                    app.version = Some(self.input.trim().to_string());
                    // Transition to tagging state
                    StateChange::Change(Box::new(super::MigrationTaggingState::new(
                        self.input.trim().to_string(),
                        app.tag_prefix.clone(),
                    )))
                } else {
                    StateChange::Keep
                }
            }
            KeyCode::Esc => {
                // Go back to results to continue reviewing PRs
                StateChange::Change(Box::new(MigrationResultsState::new()))
            }
            _ => StateChange::Keep,
        }
    }
}