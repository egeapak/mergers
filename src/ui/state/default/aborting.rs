use super::MergeState;
use crate::{
    git,
    ui::apps::MergeApp,
    ui::state::typed::{ModeState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

/// State for aborting the cherry-pick process with background cleanup.
///
/// This state provides immediate UI feedback while running cleanup operations
/// (git cherry-pick --abort, worktree removal, branch deletion) in a background thread.
pub struct AbortingState {
    is_complete: Arc<Mutex<bool>>,
    cleanup_result: Arc<Mutex<Option<Result<(), String>>>>,
    repo_path: PathBuf,
    version: String,
    target_branch: String,
}

impl AbortingState {
    /// Create a new aborting state and immediately start the cleanup in a background thread.
    ///
    /// # Arguments
    /// * `base_repo_path` - Path to the base repository (for worktree cleanup)
    /// * `repo_path` - Path to the repository (worktree or cloned repo)
    /// * `version` - Version string used for the patch branch
    /// * `target_branch` - Target branch name
    pub fn new(
        base_repo_path: Option<PathBuf>,
        repo_path: PathBuf,
        version: String,
        target_branch: String,
    ) -> Self {
        let is_complete = Arc::new(Mutex::new(false));
        let cleanup_result = Arc::new(Mutex::new(None));

        let is_complete_clone = is_complete.clone();
        let cleanup_result_clone = cleanup_result.clone();
        let repo_path_clone = repo_path.clone();
        let version_clone = version.clone();
        let target_branch_clone = target_branch.clone();

        // Spawn a thread to run the cleanup in the background
        thread::spawn(move || {
            let result = git::cleanup_cherry_pick(
                base_repo_path.as_deref(),
                &repo_path_clone,
                &version_clone,
                &target_branch_clone,
            );

            // Store the result
            *cleanup_result_clone.lock().unwrap() = Some(result.map_err(|e| e.to_string()));
            *is_complete_clone.lock().unwrap() = true;
        });

        Self {
            is_complete,
            cleanup_result,
            repo_path,
            version,
            target_branch,
        }
    }

    #[cfg(test)]
    fn new_test(
        repo_path: PathBuf,
        version: String,
        target_branch: String,
        is_complete: bool,
        cleanup_result: Option<Result<(), String>>,
    ) -> Self {
        Self {
            is_complete: Arc::new(Mutex::new(is_complete)),
            cleanup_result: Arc::new(Mutex::new(cleanup_result)),
            repo_path,
            version,
            target_branch,
        }
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for AbortingState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, _app: &MergeApp) {
        let is_complete = *self.is_complete.lock().unwrap();

        // Main layout: Title at top, content in middle
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Instructions
            ])
            .split(f.area());

        // Title
        let (title_text, title_color) = if is_complete {
            ("✅ Abort Complete", Color::Green)
        } else {
            ("⏳ Aborting Cherry-pick Process...", Color::Yellow)
        };

        let title = Paragraph::new(title_text)
            .style(
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Content area
        let mut content_text = vec![];

        content_text.push(Line::from(""));
        content_text.push(Line::from(vec![Span::styled(
            "Cleanup Operations",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        content_text.push(Line::from(""));

        if is_complete {
            content_text.push(Line::from(vec![
                Span::styled("✓ ", Style::default().fg(Color::Green)),
                Span::raw("Aborted cherry-pick in progress"),
            ]));
            content_text.push(Line::from(vec![
                Span::styled("✓ ", Style::default().fg(Color::Green)),
                Span::raw("Cleaned up patch branch"),
            ]));

            // Check if there was an error
            let cleanup_result = self.cleanup_result.lock().unwrap();
            if let Some(Err(ref error)) = *cleanup_result {
                content_text.push(Line::from(""));
                content_text.push(Line::from(vec![Span::styled(
                    format!("Note: {}", error),
                    Style::default().fg(Color::Gray),
                )]));
            }
        } else {
            content_text.push(Line::from(vec![
                Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                Span::raw("Aborting cherry-pick in progress..."),
            ]));
            content_text.push(Line::from(vec![
                Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                Span::raw("Cleaning up patch branch..."),
            ]));
        }

        content_text.push(Line::from(""));
        content_text.push(Line::from("─────────────────────"));
        content_text.push(Line::from(""));

        content_text.push(Line::from(vec![Span::styled(
            "Details",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        content_text.push(Line::from(""));
        content_text.push(Line::from(vec![
            Span::raw("Repository: "),
            Span::styled(
                format!("{}", self.repo_path.display()),
                Style::default().fg(Color::Cyan),
            ),
        ]));
        content_text.push(Line::from(vec![
            Span::raw("Branch: "),
            Span::styled(
                format!("patch/{}-{}", self.target_branch, self.version),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        let content = Paragraph::new(content_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Abort Progress"),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(content, main_chunks[1]);

        // Instructions
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let instructions_lines = if is_complete {
            vec![Line::from(vec![
                Span::raw("Press "),
                Span::styled("any key", key_style),
                Span::raw(" to continue to results..."),
            ])]
        } else {
            vec![Line::from("Please wait while cleanup is in progress...")]
        };

        let instructions_widget = Paragraph::new(instructions_lines)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .style(Style::default().fg(Color::White));
        f.render_widget(instructions_widget, main_chunks[2]);
    }

    async fn process_key(
        &mut self,
        _code: KeyCode,
        _app: &mut MergeApp,
    ) -> StateChange<MergeState> {
        let is_complete = *self.is_complete.lock().unwrap();

        if is_complete {
            // Transition to completion state when done
            StateChange::Change(MergeState::Completion(super::CompletionState::new()))
        } else {
            // Don't process keys until cleanup is complete
            StateChange::Keep
        }
    }

    fn name(&self) -> &'static str {
        "Aborting"
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

    /// # Aborting State - In Progress
    ///
    /// Tests the aborting screen while cleanup is running.
    ///
    /// ## Test Scenario
    /// - Creates an aborting state that is still processing
    /// - Renders the aborting screen
    ///
    /// ## Expected Outcome
    /// - Should display "Aborting" indicator
    /// - Should show cleanup operations in progress
    #[test]
    fn test_aborting_in_progress() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = AbortingState::new_test(
                PathBuf::from("/path/to/repo"),
                "v1.0.0".to_string(),
                "main".to_string(),
                false, // Not complete yet
                None,
            );

            harness.render_state(&mut state);
            assert_snapshot!("in_progress", harness.backend());
        });
    }

    /// # Aborting State - Complete
    ///
    /// Tests the aborting screen when cleanup is done.
    ///
    /// ## Test Scenario
    /// - Creates an aborting state that has completed
    /// - Renders the aborting screen
    ///
    /// ## Expected Outcome
    /// - Should display success message
    /// - Should show all operations complete
    #[test]
    fn test_aborting_complete() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = AbortingState::new_test(
                PathBuf::from("/path/to/repo"),
                "v1.0.0".to_string(),
                "main".to_string(),
                true, // Complete
                Some(Ok(())),
            );

            harness.render_state(&mut state);
            assert_snapshot!("complete", harness.backend());
        });
    }

    /// # Aborting State - Complete With Error
    ///
    /// Tests the aborting screen when cleanup completed with an error.
    ///
    /// ## Test Scenario
    /// - Creates an aborting state that completed with error
    /// - Renders the aborting screen
    ///
    /// ## Expected Outcome
    /// - Should display completion but note the error
    #[test]
    fn test_aborting_complete_with_error() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = AbortingState::new_test(
                PathBuf::from("/path/to/repo"),
                "v1.0.0".to_string(),
                "main".to_string(),
                true, // Complete
                Some(Err("Failed to delete branch".to_string())),
            );

            harness.render_state(&mut state);
            assert_snapshot!("complete_with_error", harness.backend());
        });
    }

    /// # Aborting State - Key Press During Processing
    ///
    /// Tests that key presses are ignored during cleanup.
    ///
    /// ## Test Scenario
    /// - Creates an aborting state that is still processing
    /// - Simulates key press
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep (ignore input)
    #[tokio::test]
    async fn test_aborting_key_press_during_processing() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = AbortingState::new_test(
            PathBuf::from("/path/to/repo"),
            "v1.0.0".to_string(),
            "main".to_string(),
            false, // Not complete
            None,
        );

        // Any key press should be ignored during processing
        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # Aborting State - Key Press After Completion
    ///
    /// Tests that any key press after completion transitions to results.
    ///
    /// ## Test Scenario
    /// - Creates an aborting state that has completed
    /// - Simulates key press
    ///
    /// ## Expected Outcome
    /// - Should transition to CompletionState
    #[tokio::test]
    async fn test_aborting_key_press_after_completion() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = AbortingState::new_test(
            PathBuf::from("/path/to/repo"),
            "v1.0.0".to_string(),
            "main".to_string(),
            true, // Complete
            Some(Ok(())),
        );

        // Any key press should transition to completion
        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Aborting State Name
    ///
    /// Tests the state name.
    ///
    /// ## Test Scenario
    /// - Creates an aborting state
    /// - Checks the name
    ///
    /// ## Expected Outcome
    /// - Should return "Aborting"
    #[test]
    fn test_aborting_state_name() {
        let state = AbortingState::new_test(
            PathBuf::from("/path/to/repo"),
            "v1.0.0".to_string(),
            "main".to_string(),
            false,
            None,
        );

        assert_eq!(state.name(), "Aborting");
    }
}
