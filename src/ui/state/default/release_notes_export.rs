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
    layout::{Alignment, Constraint, Direction, Layout, Position},
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
    cursor_pos: usize,
    default_path: String,
    phase: ReleaseNotesPhase,
    suggestions: Vec<String>,
    suggestion_index: Option<usize>,
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
            cursor_pos: 0,
            default_path,
            phase: ReleaseNotesPhase::PathInput,
            suggestions: Vec::new(),
            suggestion_index: None,
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

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        // Find the previous char boundary by iterating backwards from cursor_pos
        let prev = self.input[..self.cursor_pos]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.input.drain(prev..self.cursor_pos);
        self.cursor_pos = prev;
    }

    fn delete_char_at_cursor(&mut self) {
        if self.cursor_pos >= self.input.len() {
            return;
        }
        // Find the next char boundary after cursor_pos
        let next = self.input[self.cursor_pos..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor_pos + idx)
            .unwrap_or(self.input.len());
        self.input.drain(self.cursor_pos..next);
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        self.cursor_pos = self.input[..self.cursor_pos]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_pos >= self.input.len() {
            return;
        }
        self.cursor_pos = self.input[self.cursor_pos..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor_pos + idx)
            .unwrap_or(self.input.len());
    }

    fn clear_suggestions(&mut self) {
        self.suggestions.clear();
        self.suggestion_index = None;
    }

    fn handle_tab(&mut self, reverse: bool) {
        // If already cycling through suggestions, advance/retreat the index
        if !self.suggestions.is_empty() && self.suggestion_index.is_some() {
            let len = self.suggestions.len();
            let idx = self.suggestion_index.unwrap_or(0);
            let next_idx = if reverse {
                if idx == 0 { len - 1 } else { idx - 1 }
            } else {
                (idx + 1) % len
            };
            self.suggestion_index = Some(next_idx);
            self.input = self.suggestions[next_idx].clone();
            self.cursor_pos = self.input.len();
            return;
        }

        // Compute completions from the filesystem
        let path = Path::new(&self.input);

        let (dir, prefix) = if self.input.ends_with('/') || self.input.ends_with('\\') {
            // User typed a directory ending with separator - list its contents
            (path.to_path_buf(), "")
        } else if let Some(parent) = path.parent() {
            // Split into directory + partial filename prefix
            let file_prefix = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            (
                if parent.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    parent.to_path_buf()
                },
                file_prefix,
            )
        } else {
            // No parent - use current directory
            (PathBuf::from("."), self.input.as_str())
        };

        let entries = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };

        let mut matches: Vec<String> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name();
                let name_str = name.to_str()?;
                if !prefix.is_empty() && !name_str.starts_with(prefix) {
                    return None;
                }
                let full = dir.join(name_str);
                let mut result = full.to_string_lossy().into_owned();
                // Append '/' for directories to allow continued tab completion
                if entry.file_type().ok()?.is_dir() {
                    result.push('/');
                }
                Some(result)
            })
            .collect();

        matches.sort_unstable();

        match matches.len() {
            0 => {}
            1 => {
                // Single match - complete directly, no cycling needed
                self.input = matches.into_iter().next().unwrap();
                self.cursor_pos = self.input.len();
            }
            _ => {
                // Multiple matches - fill longest common prefix and store for cycling
                let common = longest_common_prefix(&matches);
                if common.len() > self.input.len() {
                    // We can extend the input with the common prefix
                    self.input = common;
                    self.cursor_pos = self.input.len();
                }
                self.suggestions = matches;
                self.suggestion_index = Some(0);
                self.input.clone_from(&self.suggestions[0]);
                self.cursor_pos = self.input.len();
            }
        }
    }

    fn cursor_display_pos(&self) -> usize {
        // Count the number of characters (not bytes) before cursor_pos
        self.input[..self.cursor_pos].chars().count()
    }

    fn render_path_input(&self, f: &mut Frame) {
        let has_suggestions = !self.suggestions.is_empty();

        let constraints = if has_suggestions {
            vec![
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(self.suggestions.len().min(8) as u16 + 2),
                Constraint::Min(0),
            ]
        } else {
            vec![
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints(constraints)
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

        // Set cursor position inside the input box (border offset: +1 col, +1 row)
        let cursor_x = chunks[1].x + 1 + self.cursor_display_pos() as u16;
        let cursor_y = chunks[1].y + 1;
        f.set_cursor_position(Position::new(cursor_x, cursor_y));

        // Default path info
        let default_info = Paragraph::new(Line::from(vec![
            Span::styled("Default: ", Style::default().fg(Color::Gray)),
            Span::styled(&self.default_path, Style::default().fg(Color::DarkGray)),
        ]))
        .alignment(Alignment::Center);
        f.render_widget(default_info, chunks[2]);

        // Autocomplete suggestions (only when present)
        if has_suggestions {
            let selected = self.suggestion_index.unwrap_or(0);
            let suggestion_lines: Vec<Line> = self
                .suggestions
                .iter()
                .enumerate()
                .take(8)
                .map(|(i, s)| {
                    if i == selected {
                        Line::from(Span::styled(
                            s.as_str(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ))
                    } else {
                        Line::from(Span::styled(s.as_str(), Style::default().fg(Color::Gray)))
                    }
                })
                .collect();

            let suggestions_block = Paragraph::new(suggestion_lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Suggestions (Tab to cycle)")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            f.render_widget(suggestions_block, chunks[3]);
        }

        // Help text
        let help_chunk = if has_suggestions {
            chunks[4]
        } else {
            chunks[3]
        };
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_lines = vec![Line::from(vec![
            Span::styled("Enter", key_style),
            Span::raw(": Export | "),
            Span::styled("Tab", key_style),
            Span::raw(": Autocomplete | "),
            Span::styled("←→", key_style),
            Span::raw(": Move cursor | "),
            Span::styled("Esc", key_style),
            Span::raw(": Go back"),
        ])];
        let help = Paragraph::new(help_lines)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center);
        f.render_widget(help, help_chunk);
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

fn longest_common_prefix(strings: &[String]) -> String {
    let Some(first) = strings.first() else {
        return String::new();
    };
    let mut prefix_len = first.len();
    for s in &strings[1..] {
        prefix_len = prefix_len.min(s.len());
        for (i, (a, b)) in first.bytes().zip(s.bytes()).enumerate() {
            if a != b {
                prefix_len = prefix_len.min(i);
                break;
            }
        }
    }
    // Ensure we don't split a multi-byte char
    while prefix_len > 0 && !first.is_char_boundary(prefix_len) {
        prefix_len -= 1;
    }
    first[..prefix_len].to_string()
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
                    self.clear_suggestions();
                    self.insert_char(c);
                    StateChange::Keep
                }
                KeyCode::Backspace => {
                    self.clear_suggestions();
                    self.delete_char_before_cursor();
                    StateChange::Keep
                }
                KeyCode::Delete => {
                    self.clear_suggestions();
                    self.delete_char_at_cursor();
                    StateChange::Keep
                }
                KeyCode::Left => {
                    self.clear_suggestions();
                    self.move_cursor_left();
                    StateChange::Keep
                }
                KeyCode::Right => {
                    self.clear_suggestions();
                    self.move_cursor_right();
                    StateChange::Keep
                }
                KeyCode::Home => {
                    self.clear_suggestions();
                    self.cursor_pos = 0;
                    StateChange::Keep
                }
                KeyCode::End => {
                    self.clear_suggestions();
                    self.cursor_pos = self.input.len();
                    StateChange::Keep
                }
                KeyCode::Tab => {
                    self.handle_tab(false);
                    StateChange::Keep
                }
                KeyCode::BackTab => {
                    self.handle_tab(true);
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
            state.cursor_pos = state.input.len();
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
    /// Tests that character input is accumulated at cursor position.
    ///
    /// ## Test Scenario
    /// - Types characters into the path input
    ///
    /// ## Expected Outcome
    /// - Input string should contain the typed characters
    /// - Cursor position should advance with each character
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
        assert_eq!(state.cursor_pos, 4);
    }

    /// # Release Notes Export - Backspace
    ///
    /// Tests that backspace removes the character before cursor.
    ///
    /// ## Test Scenario
    /// - Sets input with cursor at end, presses backspace
    ///
    /// ## Expected Outcome
    /// - Character before cursor should be removed
    /// - Cursor position should decrement
    #[tokio::test]
    async fn test_release_notes_export_backspace() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp/test".to_string();
        state.cursor_pos = state.input.len();

        ModeState::process_key(&mut state, KeyCode::Backspace, harness.merge_app_mut()).await;

        assert_eq!(state.input, "/tmp/tes");
        assert_eq!(state.cursor_pos, 8);
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

    /// # Release Notes Export - Cursor Movement Left/Right
    ///
    /// Tests that left and right arrow keys move the cursor correctly.
    ///
    /// ## Test Scenario
    /// - Sets input with cursor at end
    /// - Moves cursor left twice, then right once
    ///
    /// ## Expected Outcome
    /// - Cursor should be at correct byte position after each movement
    #[tokio::test]
    async fn test_release_notes_export_cursor_movement() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp".to_string();
        state.cursor_pos = 4;

        ModeState::process_key(&mut state, KeyCode::Left, harness.merge_app_mut()).await;
        assert_eq!(state.cursor_pos, 3);

        ModeState::process_key(&mut state, KeyCode::Left, harness.merge_app_mut()).await;
        assert_eq!(state.cursor_pos, 2);

        ModeState::process_key(&mut state, KeyCode::Right, harness.merge_app_mut()).await;
        assert_eq!(state.cursor_pos, 3);
    }

    /// # Release Notes Export - Home and End Keys
    ///
    /// Tests that Home moves cursor to start and End moves to end.
    ///
    /// ## Test Scenario
    /// - Sets input with cursor in the middle
    /// - Presses Home, then End
    ///
    /// ## Expected Outcome
    /// - Home should set cursor_pos to 0
    /// - End should set cursor_pos to input length
    #[tokio::test]
    async fn test_release_notes_export_home_end() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp/test".to_string();
        state.cursor_pos = 4;

        ModeState::process_key(&mut state, KeyCode::Home, harness.merge_app_mut()).await;
        assert_eq!(state.cursor_pos, 0);

        ModeState::process_key(&mut state, KeyCode::End, harness.merge_app_mut()).await;
        assert_eq!(state.cursor_pos, 9);
    }

    /// # Release Notes Export - Insert at Cursor Position
    ///
    /// Tests that characters are inserted at the cursor position, not appended.
    ///
    /// ## Test Scenario
    /// - Sets input with cursor in the middle
    /// - Types a character
    ///
    /// ## Expected Outcome
    /// - Character should be inserted at cursor position
    /// - Cursor should advance past the inserted character
    #[tokio::test]
    async fn test_release_notes_export_insert_at_cursor() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp".to_string();
        state.cursor_pos = 1; // cursor after '/'

        ModeState::process_key(&mut state, KeyCode::Char('u'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('r'), harness.merge_app_mut()).await;

        assert_eq!(state.input, "/usrtmp");
        assert_eq!(state.cursor_pos, 4);
    }

    /// # Release Notes Export - Backspace at Cursor Mid-Position
    ///
    /// Tests that backspace removes the character before the cursor in mid-string.
    ///
    /// ## Test Scenario
    /// - Sets input with cursor in the middle
    /// - Presses backspace
    ///
    /// ## Expected Outcome
    /// - Character before cursor should be removed
    /// - Characters after cursor should remain intact
    #[tokio::test]
    async fn test_release_notes_export_backspace_mid_position() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp/test".to_string();
        state.cursor_pos = 4; // cursor after 'p', before '/'

        ModeState::process_key(&mut state, KeyCode::Backspace, harness.merge_app_mut()).await;

        assert_eq!(state.input, "/tm/test");
        assert_eq!(state.cursor_pos, 3);
    }

    /// # Release Notes Export - Delete Key
    ///
    /// Tests that Delete removes the character at the cursor position.
    ///
    /// ## Test Scenario
    /// - Sets input with cursor in the middle
    /// - Presses Delete
    ///
    /// ## Expected Outcome
    /// - Character at cursor should be removed
    /// - Cursor position should remain the same
    #[tokio::test]
    async fn test_release_notes_export_delete_key() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp/test".to_string();
        state.cursor_pos = 4; // cursor before '/'

        ModeState::process_key(&mut state, KeyCode::Delete, harness.merge_app_mut()).await;

        assert_eq!(state.input, "/tmptest");
        assert_eq!(state.cursor_pos, 4);
    }

    /// # Release Notes Export - Left at Start Does Nothing
    ///
    /// Tests that pressing Left at position 0 does not move cursor further.
    ///
    /// ## Test Scenario
    /// - Sets cursor at position 0
    /// - Presses Left
    ///
    /// ## Expected Outcome
    /// - Cursor should remain at 0
    #[tokio::test]
    async fn test_release_notes_export_left_at_start() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp".to_string();
        state.cursor_pos = 0;

        ModeState::process_key(&mut state, KeyCode::Left, harness.merge_app_mut()).await;
        assert_eq!(state.cursor_pos, 0);
    }

    /// # Release Notes Export - Right at End Does Nothing
    ///
    /// Tests that pressing Right at the end of input does not move cursor further.
    ///
    /// ## Test Scenario
    /// - Sets cursor at input end
    /// - Presses Right
    ///
    /// ## Expected Outcome
    /// - Cursor should remain at input length
    #[tokio::test]
    async fn test_release_notes_export_right_at_end() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());
        state.input = "/tmp".to_string();
        state.cursor_pos = 4;

        ModeState::process_key(&mut state, KeyCode::Right, harness.merge_app_mut()).await;
        assert_eq!(state.cursor_pos, 4);
    }

    /// # Release Notes Export - Backspace on Empty Input
    ///
    /// Tests that backspace on empty input is a no-op.
    ///
    /// ## Test Scenario
    /// - Empty input, cursor at 0
    /// - Presses Backspace
    ///
    /// ## Expected Outcome
    /// - Input should remain empty
    /// - Cursor should remain at 0
    #[tokio::test]
    async fn test_release_notes_export_backspace_empty() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = ReleaseNotesExportState::new(harness.merge_app());

        ModeState::process_key(&mut state, KeyCode::Backspace, harness.merge_app_mut()).await;

        assert_eq!(state.input, "");
        assert_eq!(state.cursor_pos, 0);
    }

    #[test]
    fn test_longest_common_prefix() {
        let strings = vec![
            "/tmp/abc".to_string(),
            "/tmp/abd".to_string(),
            "/tmp/abe".to_string(),
        ];
        assert_eq!(super::longest_common_prefix(&strings), "/tmp/ab");
    }

    #[test]
    fn test_longest_common_prefix_identical() {
        let strings = vec!["/tmp/test".to_string(), "/tmp/test".to_string()];
        assert_eq!(super::longest_common_prefix(&strings), "/tmp/test");
    }

    #[test]
    fn test_longest_common_prefix_no_common() {
        let strings = vec!["abc".to_string(), "xyz".to_string()];
        assert_eq!(super::longest_common_prefix(&strings), "");
    }

    #[test]
    fn test_longest_common_prefix_empty() {
        let strings: Vec<String> = vec![];
        assert_eq!(super::longest_common_prefix(&strings), "");
    }
}
