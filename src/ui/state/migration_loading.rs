use super::{AppState, StateChange};
use crate::ui::App;
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

#[derive(Clone)]
pub struct MigrationLoadingState {
    pub status: String,
    pub progress: f64,
    pub completed: bool,
    pub error: Option<String>,
}

impl MigrationLoadingState {
    pub fn new() -> Self {
        Self {
            status: "Initializing...".to_string(),
            progress: 0.0,
            completed: false,
            error: None,
        }
    }

    pub fn update_status(&mut self, status: &str) {
        self.status = status.to_string();
        // Update progress based on status
        if status.contains("Fetching pull requests") {
            self.progress = 0.1;
        } else if status.contains("Calculating git symmetric difference") {
            self.progress = 0.3;
        } else if status.contains("Analyzing PR") {
            // Extract progress from PR analysis
            if let Some(captures) = regex::Regex::new(r"Analyzing PR (\d+)/(\d+)")
                .unwrap()
                .captures(status) {
                if let (Ok(current), Ok(total)) = (
                    captures.get(1).unwrap().as_str().parse::<f64>(),
                    captures.get(2).unwrap().as_str().parse::<f64>(),
                ) {
                    self.progress = 0.3 + (current / total) * 0.6;
                }
            }
        } else if status.contains("Categorizing results") {
            self.progress = 0.95;
        }
    }

    pub fn complete(&mut self) {
        self.completed = true;
        self.progress = 1.0;
        self.status = "Analysis complete!".to_string();
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.status = "Error occurred during analysis".to_string();
    }
}

#[async_trait]
impl AppState for MigrationLoadingState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Title
                Constraint::Length(3),  // Progress bar
                Constraint::Length(5),  // Status
                Constraint::Min(5),     // Help/spacer
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("Migration Analysis")
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Progress bar
        let progress_bar = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(Style::default().fg(Color::Green))
            .percent((self.progress * 100.0) as u16)
            .label(format!("{:.1}%", self.progress * 100.0));
        f.render_widget(progress_bar, chunks[1]);

        // Status
        let status_color = if self.error.is_some() {
            Color::Red
        } else if self.completed {
            Color::Green
        } else {
            Color::Yellow
        };

        let status_text = if let Some(error) = &self.error {
            vec![
                Line::from(vec![
                    Span::styled("Error:", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(error.clone()),
            ]
        } else {
            vec![Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(&self.status, Style::default().fg(status_color)),
            ])]
        };

        let status_widget = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .wrap(Wrap { trim: true });
        f.render_widget(status_widget, chunks[2]);

        // Help text
        let help_text = if self.error.is_some() {
            vec![
                Line::from("Press q to quit or r to retry"),
            ]
        } else if self.completed {
            vec![
                Line::from("Analysis completed! Press any key to continue..."),
            ]
        } else {
            vec![
                Line::from("Press q to cancel analysis"),
                Line::from("Please wait while we analyze your pull requests..."),
            ]
        };

        let help_widget = Paragraph::new(help_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title("Help"));
        f.render_widget(help_widget, chunks[3]);
    }

    async fn process_key(&mut self, code: KeyCode, _app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Char('r') if self.error.is_some() => {
                // Reset for retry
                self.error = None;
                self.progress = 0.0;
                self.completed = false;
                self.status = "Retrying...".to_string();
                StateChange::Keep
            }
            _ if self.completed => {
                // Any key continues after completion
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }
}