use super::{MigrationModeState, MigrationResultsState};
use crate::{
    ui::apps::MigrationApp,
    ui::state::typed::{ModeState, StateChange},
};
use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TaggingError {
    pub pr_id: i32,
    pub pr_title: String,
    pub error: String,
}

pub struct MigrationTaggingState {
    total_prs: usize,
    tagged_prs: usize,
    current_batch: usize,
    total_batches: usize,
    errors: Vec<TaggingError>,
    is_complete: bool,
    start_time: Option<Instant>,
    tag_name: String,
    started: bool,

    // Task management
    tagging_tasks: Option<Vec<tokio::task::JoinHandle<Result<Vec<TaggingError>>>>>,
}

impl MigrationTaggingState {
    pub fn new(version: String, tag_prefix: String) -> Self {
        let tag_name = format!("{}{}", tag_prefix, version);

        Self {
            total_prs: 0,
            tagged_prs: 0,
            current_batch: 0,
            total_batches: 0,
            errors: Vec::new(),
            is_complete: false,
            start_time: None,
            tag_name,
            started: false,
            tagging_tasks: None,
        }
    }

    pub async fn start_tagging(&mut self, app: &MigrationApp) {
        if self.started {
            return;
        }
        self.started = true;
        if let Some(analysis) = &app.migration_analysis() {
            let eligible_prs = &analysis.eligible_prs;
            self.total_prs = eligible_prs.len();

            if self.total_prs == 0 {
                self.is_complete = true;
                return;
            }

            // Get batch size - we'll pass it as parameter since we don't have direct access to config here
            let batch_size = 50; // Will be passed from version input state

            self.total_batches = self.total_prs.div_ceil(batch_size);
            self.start_time = Some(Instant::now());

            // Create batches
            let batches: Vec<_> = eligible_prs
                .chunks(batch_size)
                .map(|chunk| chunk.to_vec())
                .collect();

            // Start tagging tasks
            let mut tasks = Vec::new();

            for batch in batches.into_iter() {
                let client = app.client().clone();
                let tag_name = self.tag_name.clone();

                let task = tokio::spawn(async move {
                    let mut batch_errors = Vec::new();

                    for pr in batch {
                        match client.add_label_to_pr(pr.pr.id, &tag_name).await {
                            Ok(_) => {
                                // Success - no action needed
                            }
                            Err(e) => {
                                batch_errors.push(TaggingError {
                                    pr_id: pr.pr.id,
                                    pr_title: pr.pr.title.clone(),
                                    error: e.to_string(),
                                });
                            }
                        }
                    }

                    Ok(batch_errors)
                });

                tasks.push(task);
            }

            self.tagging_tasks = Some(tasks);
        }
    }

    pub async fn check_progress(&mut self) -> bool {
        if let Some(tasks) = &mut self.tagging_tasks {
            let mut completed_count = 0;
            let mut new_errors = Vec::new();

            // Check each task
            for (i, task) in tasks.iter_mut().enumerate() {
                if task.is_finished() {
                    completed_count += 1;

                    // If this batch just completed, collect results
                    if i >= self.current_batch {
                        match task.await {
                            Ok(Ok(batch_errors)) => {
                                new_errors.extend(batch_errors);
                            }
                            Ok(Err(error)) => {
                                // Task failed entirely
                                new_errors.push(TaggingError {
                                    pr_id: 0,
                                    pr_title: format!("Batch {}", i + 1),
                                    error: error.to_string(),
                                });
                            }
                            Err(e) => {
                                // Task panicked
                                new_errors.push(TaggingError {
                                    pr_id: 0,
                                    pr_title: format!("Batch {}", i + 1),
                                    error: format!("Task failed: {}", e),
                                });
                            }
                        }

                        self.current_batch = i + 1;
                        // Estimate tagged PRs based on completed batches
                        let batch_size = if self.total_prs > 0 && self.total_batches > 0 {
                            self.total_prs.div_ceil(self.total_batches)
                        } else {
                            50
                        };
                        self.tagged_prs =
                            std::cmp::min(self.current_batch * batch_size, self.total_prs);
                    }
                }
            }

            // Add new errors
            self.errors.extend(new_errors);

            // Check if all tasks are complete
            if completed_count == self.total_batches {
                self.is_complete = true;
                self.tagged_prs = self.total_prs; // Ensure final count is correct
                return true;
            }
        }

        false
    }

    fn render_progress(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let progress = if self.total_prs > 0 {
            (self.tagged_prs as f64 / self.total_prs as f64) * 100.0
        } else {
            100.0
        };

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Tagging Progress"),
            )
            .gauge_style(Style::default().fg(Color::Green))
            .percent(progress as u16)
            .label(format!("Tagged {}/{} PRs", self.tagged_prs, self.total_prs));

        f.render_widget(gauge, area);
    }

    fn render_status(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let status_text = if self.is_complete {
            let successfully_tagged = self.total_prs - self.errors.len();
            if self.errors.is_empty() {
                vec![
                    Line::from(vec![Span::styled(
                        "✅ Migration Complete!",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        format!("Tag: '{}'", self.tag_name),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Total PRs: ", Style::default().fg(Color::Gray)),
                        Span::styled(
                            format!("{}", self.total_prs),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("✅ Tagged: ", Style::default().fg(Color::Gray)),
                        Span::styled(
                            format!("{}", successfully_tagged),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("❌ Failed: ", Style::default().fg(Color::Gray)),
                        Span::styled("0", Style::default().fg(Color::Green)),
                    ]),
                ]
            } else {
                vec![
                    Line::from(vec![Span::styled(
                        "⚠️  Migration Complete with Errors",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        format!("Tag: '{}'", self.tag_name),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Total PRs: ", Style::default().fg(Color::Gray)),
                        Span::styled(
                            format!("{}", self.total_prs),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("✅ Tagged: ", Style::default().fg(Color::Gray)),
                        Span::styled(
                            format!("{}", successfully_tagged),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("❌ Failed: ", Style::default().fg(Color::Gray)),
                        Span::styled(
                            format!("{}", self.errors.len()),
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                    ]),
                ]
            }
        } else {
            let elapsed = self
                .start_time
                .map(|start| start.elapsed().as_secs())
                .unwrap_or(0);
            vec![
                Line::from(vec![Span::styled(
                    "🏃 Tagging in Progress...",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        format!("Batch: {}/{}", self.current_batch, self.total_batches),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(" | ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("Time: {}s", elapsed),
                        Style::default().fg(Color::Gray),
                    ),
                ]),
                Line::from(vec![Span::styled(
                    format!("Tag: '{}'", self.tag_name),
                    Style::default().fg(Color::Cyan),
                )]),
            ]
        };

        let status = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .alignment(Alignment::Center);

        f.render_widget(status, area);
    }

    fn render_errors(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        if self.errors.is_empty() {
            let no_errors = Paragraph::new(vec![Line::from(vec![Span::styled(
                "No errors",
                Style::default().fg(Color::Green),
            )])])
            .block(Block::default().borders(Borders::ALL).title("Errors"))
            .alignment(Alignment::Center);

            f.render_widget(no_errors, area);
        } else {
            let error_lines: Vec<Line> = self
                .errors
                .iter()
                .map(|err| {
                    Line::from(vec![
                        Span::styled(format!("#{}", err.pr_id), Style::default().fg(Color::Red)),
                        Span::raw(" "),
                        Span::styled(&err.pr_title, Style::default().fg(Color::White)),
                        Span::raw(": "),
                        Span::styled(&err.error, Style::default().fg(Color::Gray)),
                    ])
                })
                .collect();

            let errors = Paragraph::new(error_lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("Errors ({})", self.errors.len()))
                        .border_style(Style::default().fg(Color::Red)),
                )
                .wrap(Wrap { trim: true });

            f.render_widget(errors, area);
        }
    }

    fn render_help(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_text = if self.is_complete {
            vec![
                Line::from(vec![Span::styled(
                    "Migration tagging completed!",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("any key", key_style),
                    Span::raw(" to return to results | "),
                    Span::styled("q", key_style),
                    Span::raw(" to quit"),
                ]),
            ]
        } else {
            vec![
                Line::from(vec![Span::styled(
                    "Tagging PRs in parallel batches...",
                    Style::default().fg(Color::Yellow),
                )]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Please wait for completion",
                    Style::default().fg(Color::Gray),
                )]),
            ]
        };

        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .alignment(Alignment::Center);

        f.render_widget(help, area);
    }
}

// ============================================================================
// AppState Implementation
// ============================================================================

#[async_trait]
impl ModeState for MigrationTaggingState {
    type Mode = MigrationModeState;

    fn ui(&mut self, f: &mut Frame, _app: &MigrationApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),  // Progress bar
                Constraint::Length(10), // Status (increased for statistics)
                Constraint::Min(4),     // Errors
                Constraint::Length(4),  // Help (increased for better formatting)
            ])
            .split(f.area());

        self.render_progress(f, chunks[0]);
        self.render_status(f, chunks[1]);
        self.render_errors(f, chunks[2]);
        self.render_help(f, chunks[3]);
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MigrationApp,
    ) -> StateChange<MigrationModeState> {
        match code {
            KeyCode::Char('q') if self.is_complete => StateChange::Exit,
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Null => {
                // Auto-start tagging and check progress (but don't transition)
                if !self.is_complete {
                    if !self.started {
                        // Start tagging automatically
                        self.start_tagging(app).await;
                    } else {
                        // Check if tagging is complete
                        self.check_progress().await;
                    }
                }
                StateChange::Keep
            }
            _ if self.is_complete => {
                // Any actual key press returns to results when complete
                StateChange::Change(MigrationModeState::Results(MigrationResultsState::new()))
            }
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "MigrationTagging"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{TuiTestHarness, create_test_config_migration},
    };
    use insta::assert_snapshot;

    /// # Migration Tagging State - In Progress
    ///
    /// Tests the migration tagging screen during tagging.
    ///
    /// ## Test Scenario
    /// - Creates a migration tagging state
    /// - Sets up eligible PRs for tagging
    /// - Renders the tagging progress display
    ///
    /// ## Expected Outcome
    /// - Should display tagging progress
    /// - Should show PR list with status
    /// - Should display progress indicators
    #[test]
    fn test_migration_tagging_in_progress() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            harness.app.set_version(Some("v1.0.0".to_string()));

            let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());
            state.total_prs = 3;
            state.tagged_prs = 1;
            let mut mode_state = MigrationModeState::Tagging(state);
            harness.render_migration_state(&mut mode_state);

            assert_snapshot!("in_progress", harness.backend());
        });
    }

    /// # Migration Tagging State - Success
    ///
    /// Tests the migration tagging screen after successful completion.
    ///
    /// ## Test Scenario
    /// - Creates a migration tagging state
    /// - Sets state to complete with all PRs tagged successfully
    /// - Renders the success display
    ///
    /// ## Expected Outcome
    /// - Should display "Migration Complete!" message
    /// - Should show statistics: Total PRs, Tagged count, Failed count (0)
    /// - Should show tag name
    /// - Should display help text for user to continue
    #[test]
    fn test_migration_tagging_success() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            harness.app.set_version(Some("v1.0.0".to_string()));

            let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());
            state.total_prs = 5;
            state.tagged_prs = 5;
            state.is_complete = true;
            let mut mode_state = MigrationModeState::Tagging(state);
            harness.render_migration_state(&mut mode_state);

            assert_snapshot!("success", harness.backend());
        });
    }

    /// # Migration Tagging State - Success with Errors
    ///
    /// Tests the migration tagging screen after completion with errors.
    ///
    /// ## Test Scenario
    /// - Creates a migration tagging state
    /// - Sets state to complete with some PRs failed
    /// - Adds error details for failed PRs
    /// - Renders the completion display with errors
    ///
    /// ## Expected Outcome
    /// - Should display "Migration Complete with Errors" message
    /// - Should show statistics: Total PRs, Tagged count, Failed count
    /// - Should show tag name
    /// - Should list error details for failed PRs
    /// - Should display help text for user to continue
    #[test]
    fn test_migration_tagging_success_with_errors() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            harness.app.set_version(Some("v1.0.0".to_string()));

            let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());
            state.total_prs = 5;
            state.tagged_prs = 5;
            state.is_complete = true;
            state.errors = vec![
                TaggingError {
                    pr_id: 123,
                    pr_title: "Fix authentication bug".to_string(),
                    error: "Network timeout".to_string(),
                },
                TaggingError {
                    pr_id: 456,
                    pr_title: "Update dependencies".to_string(),
                    error: "Permission denied".to_string(),
                },
            ];
            let mut mode_state = MigrationModeState::Tagging(state);
            harness.render_migration_state(&mut mode_state);

            assert_snapshot!("success_with_errors", harness.backend());
        });
    }

    /// # Migration Tagging State - Quit Key
    ///
    /// Tests 'q' key to exit.
    ///
    /// ## Test Scenario
    /// - Processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_migration_tagging_quit() {
        let config = create_test_config_migration();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.migration_app_mut())
                .await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Migration Tagging State - Quit Key When Complete
    ///
    /// Tests 'q' key to exit when tagging is complete.
    ///
    /// ## Test Scenario
    /// - Sets is_complete to true
    /// - Processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_migration_tagging_quit_when_complete() {
        let config = create_test_config_migration();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());
        state.is_complete = true;

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.migration_app_mut())
                .await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Migration Tagging State - Any Key When Complete
    ///
    /// Tests any key when tagging is complete returns to results.
    ///
    /// ## Test Scenario
    /// - Sets is_complete to true
    /// - Processes any key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to MigrationResultsState
    #[tokio::test]
    async fn test_migration_tagging_any_key_when_complete() {
        let config = create_test_config_migration();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());
        state.is_complete = true;

        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.migration_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Migration Tagging State - Other Keys During Tagging
    ///
    /// Tests other keys are ignored during tagging.
    ///
    /// ## Test Scenario
    /// - Tagging not complete
    /// - Processes various keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_migration_tagging_other_keys() {
        let config = create_test_config_migration();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());

        for key in [KeyCode::Enter, KeyCode::Up, KeyCode::Down] {
            let result = ModeState::process_key(&mut state, key, harness.migration_app_mut()).await;
            assert!(matches!(result, StateChange::Keep));
        }
    }

    /// # Migration Tagging State - Zero Progress
    ///
    /// Tests tagging at zero progress (just started).
    ///
    /// ## Test Scenario
    /// - Sets up state at 0% progress
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display 0% progress bar
    #[test]
    fn test_migration_tagging_zero_progress() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());
            state.total_prs = 10;
            state.tagged_prs = 0;
            let mut mode_state = MigrationModeState::Tagging(state);
            harness.render_migration_state(&mut mode_state);

            assert_snapshot!("zero_progress", harness.backend());
        });
    }

    /// # Migration Tagging State - Half Progress
    ///
    /// Tests tagging at 50% progress.
    ///
    /// ## Test Scenario
    /// - Sets up state at 50% progress
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display 50% progress bar
    #[test]
    fn test_migration_tagging_half_progress() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = MigrationTaggingState::new("v1.0.0".to_string(), "merged/".to_string());
            state.total_prs = 10;
            state.tagged_prs = 5;
            let mut mode_state = MigrationModeState::Tagging(state);
            harness.render_migration_state(&mut mode_state);

            assert_snapshot!("half_progress", harness.backend());
        });
    }
}
