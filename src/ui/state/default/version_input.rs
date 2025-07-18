use super::SetupRepoState;
use crate::{
    ui::App,
    ui::state::{AppState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

pub struct VersionInputState {
    input: String,
}

impl VersionInputState {
    pub fn new() -> Self {
        Self {
            input: String::new(),
        }
    }
}

#[async_trait]
impl AppState for VersionInputState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(f.area());

        let title = Paragraph::new("Enter Version Number")
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        let input_block = Paragraph::new(self.input.as_str())
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL).title("Version"));
        f.render_widget(input_block, chunks[1]);

        let help = Paragraph::new("Type version number and press Enter | Esc to go back")
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(help, chunks[2]);
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
                if !self.input.is_empty() {
                    app.version = Some(self.input.clone());
                    StateChange::Change(Box::new(SetupRepoState::new()))
                } else {
                    StateChange::Keep
                }
            }
            KeyCode::Esc => StateChange::Change(Box::new(super::PullRequestSelectionState::new())),
            _ => StateChange::Keep,
        }
    }
}
