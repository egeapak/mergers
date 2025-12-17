use super::{MergeState, SetupRepoState};
use crate::{
    ui::apps::MergeApp,
    ui::state::typed::{TypedAppState, TypedStateChange},
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

impl Default for VersionInputState {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionInputState {
    pub fn new() -> Self {
        Self {
            input: String::new(),
        }
    }
}

// ============================================================================
// TypedAppState Implementation (Primary)
// ============================================================================

#[async_trait]
impl TypedAppState for VersionInputState {
    type App = MergeApp;
    type StateEnum = MergeState;

    fn ui(&mut self, f: &mut Frame, _app: &MergeApp) {
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

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MergeApp,
    ) -> TypedStateChange<MergeState> {
        match code {
            KeyCode::Char(c) => {
                self.input.push(c);
                TypedStateChange::Keep
            }
            KeyCode::Backspace => {
                self.input.pop();
                TypedStateChange::Keep
            }
            KeyCode::Enter => {
                if !self.input.is_empty() {
                    app.set_version(Some(self.input.clone()));
                    TypedStateChange::Change(MergeState::SetupRepo(SetupRepoState::new()))
                } else {
                    TypedStateChange::Keep
                }
            }
            KeyCode::Esc => TypedStateChange::Change(MergeState::PullRequestSelection(
                super::PullRequestSelectionState::new(),
            )),
            _ => TypedStateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "VersionInput"
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

    /// # Version Input State - Empty Input
    ///
    /// Tests the version input screen with no input yet.
    ///
    /// ## Test Scenario
    /// - Creates a new version input state
    /// - Renders the state with empty input
    ///
    /// ## Expected Outcome
    /// - Should display "Enter Version Number" title
    /// - Should show empty input box with "Version" label
    /// - Should display help text with instructions
    #[test]
    fn test_version_input_empty() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = MergeState::VersionInput(VersionInputState::new());
            harness.render_merge_state(&mut state);

            assert_snapshot!("empty_input", harness.backend());
        });
    }

    /// # Version Input State - With Input
    ///
    /// Tests the version input screen with version number entered.
    ///
    /// ## Test Scenario
    /// - Creates a version input state
    /// - Sets input to a version number
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display the version number in the input box
    /// - Should maintain consistent layout
    #[test]
    fn test_version_input_with_version() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = VersionInputState::new();
            inner_state.input = "v1.2.3".to_string();
            let mut state = MergeState::VersionInput(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("with_version", harness.backend());
        });
    }

    /// # Version Input State - With Long Input
    ///
    /// Tests the version input screen with a longer version string.
    ///
    /// ## Test Scenario
    /// - Creates a version input state
    /// - Sets input to a longer version number
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display the full version number
    /// - Input box should handle the longer text
    #[test]
    fn test_version_input_with_long_version() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = VersionInputState::new();
            inner_state.input = "v2.5.0-alpha.1+build.123".to_string();
            let mut state = MergeState::VersionInput(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("with_long_version", harness.backend());
        });
    }
}
