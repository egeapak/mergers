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
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub struct ErrorState;

impl Default for ErrorState {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorState {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AppState for ErrorState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area());

        let title = Paragraph::new("❌ Error Occurred")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        let error_msg = app.error_message.as_deref().unwrap_or("Unknown error");
        let error = Paragraph::new(error_msg)
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(error, chunks[1]);

        let help = Paragraph::new("Press 'q' to exit")
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(help, chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, _app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{TuiTestHarness, create_test_config_default},
    };
    use insta::assert_snapshot;

    /// # Error State with Message
    ///
    /// Tests the error state display with a specific error message.
    ///
    /// ## Test Scenario
    /// - Creates an error state
    /// - Sets a sample error message in the app context
    /// - Renders the error display screen in a fixed 80x30 terminal
    ///
    /// ## Expected Outcome
    /// - Should display "❌ Error Occurred" title in red
    /// - Should show the error message in a bordered box
    /// - Should display "Press 'q' to exit" instruction at the bottom
    #[test]
    fn test_error_state_with_message() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            // Set error message
            harness.app.error_message = Some(
                "Failed to connect to Azure DevOps API. Please check your credentials.".to_string(),
            );

            let state = Box::new(ErrorState::new());
            harness.render_state(state);

            assert_snapshot!("error_with_message", harness.backend());
        });
    }

    /// # Error State without Message
    ///
    /// Tests the error state display when no specific error message is available.
    ///
    /// ## Test Scenario
    /// - Creates an error state
    /// - Does not set any error message (None)
    /// - Renders the error display screen
    ///
    /// ## Expected Outcome
    /// - Should display "❌ Error Occurred" title
    /// - Should show the fallback message "Unknown error"
    /// - Should display exit instruction
    #[test]
    fn test_error_state_without_message() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            // No error message set
            harness.app.error_message = None;

            let state = Box::new(ErrorState::new());
            harness.render_state(state);

            assert_snapshot!("error_without_message", harness.backend());
        });
    }

    /// # Error State with Long Message
    ///
    /// Tests the error state display with a long error message that requires wrapping.
    ///
    /// ## Test Scenario
    /// - Creates an error state
    /// - Sets a very long error message
    /// - Renders the error display screen
    ///
    /// ## Expected Outcome
    /// - Should display the error message with proper text wrapping
    /// - Should maintain the same layout structure
    /// - Text should wrap within the bordered box
    #[test]
    fn test_error_state_with_long_message() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            harness.app.error_message = Some(
                "An unexpected error occurred while processing your request. The Azure DevOps API returned a 401 Unauthorized error, which typically indicates that the Personal Access Token (PAT) is invalid, expired, or does not have sufficient permissions to access the requested resource. Please verify your credentials and try again."
                    .to_string(),
            );

            let state = Box::new(ErrorState::new());
            harness.render_state(state);

            assert_snapshot!("error_with_long_message", harness.backend());
        });
    }
}
