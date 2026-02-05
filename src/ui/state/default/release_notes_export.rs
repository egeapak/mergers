use crate::{
    core::state::MergeStatus,
    release_notes,
    ui::apps::MergeApp,
    ui::state::default::{CompletionState, MergeState},
    ui::state::typed::{ModeState, StateChange},
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
use std::path::{Path, PathBuf};

enum ReleaseNotesPhase {
    PathInput,
    Success(PathBuf),
    Error(String),
}

pub struct ReleaseNotesExportState {
    input: String,
    default_path: String,
    phase: ReleaseNotesPhase,
}

impl ReleaseNotesExportState {
    pub fn new(app: &MergeApp) -> Self {
        let version = app.version.as_deref().unwrap_or("unknown");

        let sanitized_version = version.replace(['/', '\\', ' '], "_");

        let base_dir = app
            .repo_path()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let default_path = base_dir
            .join(format!("release_notes_{sanitized_version}.md"))
            .to_string_lossy()
            .to_string();

        Self {
            input: String::new(),
            default_path,
            phase: ReleaseNotesPhase::PathInput,
        }
    }

    fn resolve_path(&self) -> PathBuf {
        if self.input.trim().is_empty() {
            PathBuf::from(&self.default_path)
        } else {
            PathBuf::from(self.input.trim())
        }
    }

    fn generate_and_write(&self, app: &MergeApp) -> Result<PathBuf, String> {
        let path = self.resolve_path();

        // Ensure parent directory exists
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }

        let version = app.version.as_deref().unwrap_or("unknown");
        let content = release_notes::generate_from_merge_data(
            version,
            &app.cherry_pick_items,
            &app.pull_requests,
            app.organization(),
            app.project(),
        );

        std::fs::write(&path, &content).map_err(|e| format!("Failed to write file: {e}"))?;

        Ok(path)
    }

    fn render_path_input(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(f.area());

        let title = Paragraph::new("📝 Export Release Notes")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Input box: show typed text or default path as placeholder
        let input_content = if self.input.is_empty() {
            Line::from(Span::styled(
                &self.default_path,
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::styled(&self.input, Style::default().fg(Color::White)))
        };

        let input_block = Paragraph::new(input_content).block(
            Block::default()
                .borders(Borders::ALL)
                .title("File Path")
                .border_style(Style::default().fg(Color::Yellow)),
        );
        f.render_widget(input_block, chunks[1]);

        // Default path info
        let default_info = Paragraph::new(Line::from(vec![
            Span::styled("Default: ", Style::default().fg(Color::Gray)),
            Span::styled(&self.default_path, Style::default().fg(Color::DarkGray)),
        ]))
        .alignment(Alignment::Center);
        f.render_widget(default_info, chunks[2]);

        // Help text
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_lines = vec![Line::from(vec![
            Span::styled("Enter", key_style),
            Span::raw(": Export (default path) | "),
            Span::raw("Type path + "),
            Span::styled("Enter", key_style),
            Span::raw(": Export to custom path | "),
            Span::styled("Esc", key_style),
            Span::raw(": Go back"),
        ])];
        let help = Paragraph::new(help_lines)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center);
        f.render_widget(help, chunks[3]);
    }

    fn render_success(&self, f: &mut Frame, path: &Path) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Min(0),
            ])
            .split(f.area());

        let title = Paragraph::new("✅ Release Notes Exported Successfully")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        let info_lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("File: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    path.to_string_lossy().to_string(),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(""),
        ];
        let info = Paragraph::new(info_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Export Details"),
            )
            .alignment(Alignment::Center);
        f.render_widget(info, chunks[1]);

        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_lines = vec![Line::from(vec![
            Span::styled("Enter", key_style),
            Span::raw(" / "),
            Span::styled("Esc", key_style),
            Span::raw(": Go back to summary | "),
            Span::styled("q", key_style),
            Span::raw(": Exit"),
        ])];
        let help = Paragraph::new(help_lines)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center);
        f.render_widget(help, chunks[2]);
    }

    fn render_error(&self, f: &mut Frame, message: &str) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Min(0),
            ])
            .split(f.area());

        let title = Paragraph::new("❌ Export Failed")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        let error_lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                message.to_string(),
                Style::default().fg(Color::Red),
            )),
            Line::from(""),
        ];
        let error = Paragraph::new(error_lines)
            .block(Block::default().borders(Borders::ALL).title("Error"))
            .wrap(Wrap { trim: true });
        f.render_widget(error, chunks[1]);

        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_lines = vec![Line::from(vec![
            Span::styled("Enter", key_style),
            Span::raw(" / "),
            Span::styled("Esc", key_style),
            Span::raw(": Go back to summary | "),
            Span::styled("q", key_style),
            Span::raw(": Exit"),
        ])];
        let help = Paragraph::new(help_lines)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center);
        f.render_widget(help, chunks[2]);
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for ReleaseNotesExportState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, _app: &MergeApp) {
        match &self.phase {
            ReleaseNotesPhase::PathInput => self.render_path_input(f),
            ReleaseNotesPhase::Success(path) => self.render_success(f, path),
            ReleaseNotesPhase::Error(msg) => self.render_error(f, msg),
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> StateChange<MergeState> {
        match &self.phase {
            ReleaseNotesPhase::PathInput => match code {
                KeyCode::Char(c) => {
                    self.input.push(c);
                    StateChange::Keep
                }
                KeyCode::Backspace => {
                    self.input.pop();
                    StateChange::Keep
                }
                KeyCode::Enter => {
                    match self.generate_and_write(app) {
                        Ok(path) => self.phase = ReleaseNotesPhase::Success(path),
                        Err(msg) => self.phase = ReleaseNotesPhase::Error(msg),
                    }
                    StateChange::Keep
                }
                KeyCode::Esc => StateChange::Change(MergeState::Completion(CompletionState::new())),
                _ => StateChange::Keep,
            },
            ReleaseNotesPhase::Success(_) | ReleaseNotesPhase::Error(_) => match code {
                KeyCode::Enter | KeyCode::Esc => {
                    StateChange::Change(MergeState::Completion(CompletionState::new()))
                }
                KeyCode::Char('q') => {
                    app.with_state_file_mut(|state_file| {
                        state_file.final_status = Some(MergeStatus::Success);
                        state_file.completed_at = Some(chrono::Utc::now());
                        let _ = state_file.save_for_repo();
                    });
                    let _ = app.cleanup_state_file();
                    StateChange::Exit
                }
                _ => StateChange::Keep,
            },
        }
    }

    fn name(&self) -> &'static str {
        "ReleaseNotesExport"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{
            TuiTestHarness, create_test_cherry_pick_items, create_test_config_default,
            create_test_pull_requests,
        },
    };
    use insta::assert_snapshot;
    use std::path::PathBuf;

    /// # Release Notes Export - Empty Path Input
    ///
    /// Tests the path input screen with no input yet.
    ///
    /// ## Test Scenario
    /// - Creates a new ReleaseNotesExportState
    /// - Renders with empty input showing default path as placeholder
    ///
    /// ## Expected Outcome
    /// - Should display title, input box with dimmed default path, and help text
    #[test]
    fn test_release_notes_export_empty_input() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));

            let mut state = ReleaseNotesExportState::new(harness.merge_app());
            harness.render_state(&mut state);

            assert_snapshot!("empty_input", harness.backend());
        });
    }

    /// # Release Notes Export - With Typed Path
    ///
    /// Tests the path input screen with a custom path typed.
    ///
    /// ## Test Scenario
    /// - Creates a ReleaseNotesExportState with typed input
    /// - Renders the state with custom path visible
    ///
    /// ## Expected Outcome
    /// - Should display the typed path in white instead of dimmed default
    #[test]
    fn test_release_notes_export_with_input() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));

            let mut state = ReleaseNotesExportState::new(harness.merge_app());
            state.input = "/custom/path/release_notes.md".to_string();
            harness.render_state(&mut state);

            assert_snapshot!("with_input", harness.backend());
        });
    }

    /// # Release Notes Export - Success Phase
    ///
    /// Tests the success screen after file creation.
    ///
    /// ## Test Scenario
    /// - Creates a ReleaseNotesExportState in Success phase
    /// - Renders the success notification
    ///
    /// ## Expected Outcome
    /// - Should display success title, file path, and help text
    #[test]
    fn test_release_notes_export_success() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));

            let mut state = ReleaseNotesExportState::new(harness.merge_app());
            state.phase =
                ReleaseNotesPhase::Success(PathBuf::from("/path/to/repo/release_notes_v1.0.0.md"));
            harness.render_state(&mut state);

            assert_snapshot!("success", harness.backend());
        });
    }

    /// # Release Notes Export - Error Phase
    ///
    /// Tests the error screen when file creation fails.
    ///
    /// ## Test Scenario
    /// - Creates a ReleaseNotesExportState in Error phase
    /// - Renders the error notification
    ///
    /// ## Expected Outcome
    /// - Should display error title, error message, and help text
    #[test]
    fn test_release_notes_export_error() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));

            let mut state = ReleaseNotesExportState::new(harness.merge_app());
            state.phase =
                ReleaseNotesPhase::Error("Failed to write file: Permission denied".to_string());
            harness.render_state(&mut state);

            assert_snapshot!("error", harness.backend());
        });
    }

    /// # Release Notes Export - Esc on Path Input
    ///
    /// Tests that Esc on path input goes back to CompletionState.
    ///
    /// ## Test Scenario
    /// - Processes Esc key on PathInput phase
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to CompletionState
    #[tokio::test]
    async fn test_release_notes_export_esc_goes_back() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());

        let result =
            ModeState::process_key(&mut state, KeyCode::Esc, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Release Notes Export - Char Input
    ///
    /// Tests that character input is accumulated.
    ///
    /// ## Test Scenario
    /// - Types characters into the path input
    ///
    /// ## Expected Outcome
    /// - Input string should contain the typed characters
    #[tokio::test]
    async fn test_release_notes_export_char_input() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());

        ModeState::process_key(&mut state, KeyCode::Char('/'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('t'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('m'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('p'), harness.merge_app_mut()).await;

        assert_eq!(state.input, "/tmp");
    }

    /// # Release Notes Export - Backspace
    ///
    /// Tests that backspace removes the last character.
    ///
    /// ## Test Scenario
    /// - Types some characters then presses backspace
    ///
    /// ## Expected Outcome
    /// - Last character should be removed
    #[tokio::test]
    async fn test_release_notes_export_backspace() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp/test".to_string();

        ModeState::process_key(&mut state, KeyCode::Backspace, harness.merge_app_mut()).await;

        assert_eq!(state.input, "/tmp/tes");
    }

    /// # Release Notes Export - Enter on Success Goes Back
    ///
    /// Tests that Enter on success phase goes back to CompletionState.
    ///
    /// ## Test Scenario
    /// - State is in Success phase, processes Enter key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to CompletionState
    #[tokio::test]
    async fn test_release_notes_export_enter_on_success_goes_back() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.phase = ReleaseNotesPhase::Success(PathBuf::from("/tmp/test.md"));

        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Release Notes Export - Quit on Success
    ///
    /// Tests that 'q' on success phase exits the application.
    ///
    /// ## Test Scenario
    /// - State is in Success phase, processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_release_notes_export_quit_on_success() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.phase = ReleaseNotesPhase::Success(PathBuf::from("/tmp/test.md"));

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Release Notes Export - Enter on Error Goes Back
    ///
    /// Tests that Enter on error phase goes back to CompletionState.
    ///
    /// ## Test Scenario
    /// - State is in Error phase, processes Enter key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to CompletionState
    #[tokio::test]
    async fn test_release_notes_export_enter_on_error_goes_back() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.phase = ReleaseNotesPhase::Error("Some error".to_string());

        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Release Notes Export - Default Path Computation
    ///
    /// Tests that default path is computed correctly from app state.
    ///
    /// ## Test Scenario
    /// - Creates state with version and repo path
    ///
    /// ## Expected Outcome
    /// - Default path should be {repo_path}/release_notes_{version}.md
    #[test]
    fn test_release_notes_export_default_path() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));
        harness
            .app
            .set_repo_path(Some(PathBuf::from("/path/to/repo")));

        let state = ReleaseNotesExportState::new(harness.merge_app());
        assert_eq!(state.default_path, "/path/to/repo/release_notes_v1.0.0.md");
    }

    /// # Release Notes Export - Version Sanitization
    ///
    /// Tests that special characters in version are sanitized for filename.
    ///
    /// ## Test Scenario
    /// - Creates state with version containing special characters
    ///
    /// ## Expected Outcome
    /// - Slashes and spaces should be replaced with underscores
    #[test]
    fn test_release_notes_export_version_sanitization() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness
            .app
            .set_version(Some("feature/v1.0.0 beta".to_string()));
        harness.app.set_repo_path(Some(PathBuf::from("/repo")));

        let state = ReleaseNotesExportState::new(harness.merge_app());
        assert_eq!(
            state.default_path,
            "/repo/release_notes_feature_v1.0.0_beta.md"
        );
    }

    /// # Release Notes Export - Generate Content
    ///
    /// Tests that release notes content is generated correctly.
    ///
    /// ## Test Scenario
    /// - Creates app with cherry-pick items and pull requests
    /// - Generates release notes markdown
    ///
    /// ## Expected Outcome
    /// - Markdown should contain version header, work items, and PR summary
    #[test]
    fn test_generate_from_merge_data_content() {
        let items = create_test_cherry_pick_items();
        let prs = create_test_pull_requests();

        let content = release_notes::generate_from_merge_data(
            "v1.0.0",
            &items,
            &prs,
            "test-org",
            "test-project",
        );

        assert!(content.contains("# Release Notes - v1.0.0"));
        assert!(content.contains("**Release Date:**"));
        assert!(content.contains("work item(s) included in this release"));
    }
}
