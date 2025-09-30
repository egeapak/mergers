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
    use crate::ui::testing::*;
    use insta::assert_snapshot;

    /// # Error State With Message Test
    ///
    /// Tests the error state displaying a specific error message.
    ///
    /// ## Test Scenario
    /// - Creates an error state with a specific error message about Azure DevOps API connectivity
    /// - Renders the error screen in a fixed 80x30 terminal
    /// - Captures the complete UI output for snapshot comparison
    ///
    /// ## Expected Outcome
    /// - Should display "❌ Error Occurred" title centered in red
    /// - Should show the error message in a bordered box
    /// - Should display "Press 'q' to exit" help text at the bottom
    #[test]
    fn test_error_state_with_message() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.error_message =
                Some("Connection failed: Unable to reach Azure DevOps API".to_string());
            let state = Box::new(ErrorState::new());

            harness.render_state(state);
            assert_snapshot!("with_message", harness.backend());
        });
    }

    /// # Error State With Long Message Test
    ///
    /// Tests the error state with a very long error message that should wrap.
    ///
    /// ## Test Scenario
    /// - Creates an error state with a 200+ character error message
    /// - Renders the error screen to verify text wrapping behavior
    /// - Captures the UI output showing how long messages are handled
    ///
    /// ## Expected Outcome
    /// - Should wrap the long error message within the bordered area
    /// - Should maintain readability with proper line breaks
    /// - Should not overflow the terminal boundaries
    #[test]
    fn test_error_state_with_long_message() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.error_message = Some(
                "Authentication failed: The Personal Access Token (PAT) provided is invalid or has expired. \
                Please verify that your PAT has the required permissions (Code: Read, Work Items: Read) and \
                has not been revoked. You can generate a new PAT from Azure DevOps user settings."
                    .to_string(),
            );
            let state = Box::new(ErrorState::new());

            harness.render_state(state);
            assert_snapshot!("with_long_message", harness.backend());
        });
    }

    /// # Error State No Message Test
    ///
    /// Tests the error state when no error message is provided.
    ///
    /// ## Test Scenario
    /// - Creates an error state with error_message set to None
    /// - Renders the error screen to verify fallback behavior
    /// - Captures the UI output showing the default error message
    ///
    /// ## Expected Outcome
    /// - Should display "Unknown error" as the fallback message
    /// - Should maintain the same layout as other error displays
    /// - Should still show the title and help text
    #[test]
    fn test_error_state_no_message() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.error_message = None;
            let state = Box::new(ErrorState::new());

            harness.render_state(state);
            assert_snapshot!("no_message", harness.backend());
        });
    }

    /// # Error State Multiline Error Test
    ///
    /// Tests the error state with an error message containing newlines.
    ///
    /// ## Test Scenario
    /// - Creates an error state with a multiline error message (stack trace format)
    /// - Renders the error screen to verify multiline text handling
    /// - Captures the UI output showing how newlines are preserved
    ///
    /// ## Expected Outcome
    /// - Should display all lines of the error message
    /// - Should preserve the newline structure
    /// - Should maintain formatting within the bordered area
    #[test]
    fn test_error_state_multiline_error() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.error_message = Some(
                "Git operation failed:\n\
                Command: git cherry-pick abc123\n\
                Exit code: 1\n\
                \n\
                Error: CONFLICT (content): Merge conflict in src/main.rs\n\
                Please resolve conflicts and continue."
                    .to_string(),
            );
            let state = Box::new(ErrorState::new());

            harness.render_state(state);
            assert_snapshot!("multiline_error", harness.backend());
        });
    }

    /// # Error State Special Characters Test
    ///
    /// Tests the error state with special characters and Unicode.
    ///
    /// ## Test Scenario
    /// - Creates an error state with special characters, quotes, brackets, and emojis
    /// - Renders the error screen to verify character encoding
    /// - Captures the UI output showing how special characters are displayed
    ///
    /// ## Expected Outcome
    /// - Should display all special characters correctly
    /// - Should handle Unicode characters properly
    /// - Should maintain text formatting and readability
    #[test]
    fn test_error_state_special_characters() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.error_message = Some(
                r#"Parse error: Unexpected character '"' at position 42. Expected one of: ['{', '[', 'true', 'false', 'null']. The JSON response from the API appears malformed. 🔧 Check API version compatibility."#
                    .to_string(),
            );
            let state = Box::new(ErrorState::new());

            harness.render_state(state);
            assert_snapshot!("special_characters", harness.backend());
        });
    }
}
