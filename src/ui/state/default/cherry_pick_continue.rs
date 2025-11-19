use crate::{
    models::CherryPickStatus,
    ui::App,
    ui::state::{AppState, CherryPickState, ConflictResolutionState, StateChange},
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
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

pub struct CherryPickContinueState {
    output: Arc<Mutex<Vec<String>>>,
    is_complete: Arc<Mutex<bool>>,
    success: Arc<Mutex<Option<bool>>>,
    error_message: Arc<Mutex<Option<String>>>,
    conflicted_files: Vec<String>,
}

impl CherryPickContinueState {
    pub fn new(conflicted_files: Vec<String>, repo_path: std::path::PathBuf) -> Self {
        let output = Arc::new(Mutex::new(Vec::new()));
        let is_complete = Arc::new(Mutex::new(false));
        let success = Arc::new(Mutex::new(None));
        let error_message = Arc::new(Mutex::new(None));

        let output_clone = output.clone();
        let is_complete_clone = is_complete.clone();
        let success_clone = success.clone();
        let error_message_clone = error_message.clone();

        // Spawn a thread to run the git cherry-pick --continue command
        thread::spawn(move || {
            let mut child = match Command::new("git")
                .current_dir(&repo_path)
                .args(["cherry-pick", "--continue"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    let mut output = output_clone.lock().unwrap();
                    output.push(format!("Failed to spawn git command: {}", e));
                    *is_complete_clone.lock().unwrap() = true;
                    *success_clone.lock().unwrap() = Some(false);
                    *error_message_clone.lock().unwrap() =
                        Some(format!("Failed to spawn git command: {}", e));
                    return;
                }
            };

            // Read stdout
            if let Some(stdout) = child.stdout.take() {
                use std::io::{BufRead, BufReader};
                let output_clone = output_clone.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines().map_while(Result::ok) {
                        let mut output = output_clone.lock().unwrap();
                        output.push(line);
                    }
                });
            }

            // Read stderr
            if let Some(stderr) = child.stderr.take() {
                use std::io::{BufRead, BufReader};
                let output_clone = output_clone.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().map_while(Result::ok) {
                        let mut output = output_clone.lock().unwrap();
                        output.push(line);
                    }
                });
            }

            // Wait for the command to complete
            match child.wait() {
                Ok(status) => {
                    *is_complete_clone.lock().unwrap() = true;
                    let is_success = status.success();
                    *success_clone.lock().unwrap() = Some(is_success);

                    if !is_success {
                        let output = output_clone.lock().unwrap();
                        let error_msg = if output.is_empty() {
                            "Cherry-pick --continue failed with no output".to_string()
                        } else {
                            output.join("\n")
                        };
                        *error_message_clone.lock().unwrap() = Some(error_msg);
                    }
                }
                Err(e) => {
                    *is_complete_clone.lock().unwrap() = true;
                    *success_clone.lock().unwrap() = Some(false);
                    *error_message_clone.lock().unwrap() = Some(format!("Failed to wait: {}", e));
                }
            }
        });

        Self {
            output,
            is_complete,
            success,
            error_message,
            conflicted_files,
        }
    }
}

#[async_trait]
impl AppState for CherryPickContinueState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let is_complete = *self.is_complete.lock().unwrap();
        let success = *self.success.lock().unwrap();

        // Main layout: Title at top, content in middle, help at bottom
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
        let title_text = if is_complete {
            match success {
                Some(true) => "✅ Cherry-pick Completed Successfully",
                Some(false) => "❌ Cherry-pick Failed",
                None => "Processing Cherry-pick...",
            }
        } else {
            "⏳ Processing Cherry-pick Continue..."
        };

        let title_color = if is_complete {
            match success {
                Some(true) => Color::Green,
                Some(false) => Color::Red,
                None => Color::Yellow,
            }
        } else {
            Color::Yellow
        };

        let title = Paragraph::new(title_text)
            .style(
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Split content horizontally: Left and Right panes
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);

        // Left pane: Commit output
        let output = self.output.lock().unwrap();
        let output_text: Vec<Line> = output
            .iter()
            .map(|line| Line::from(line.as_str()))
            .collect();

        let output_widget = Paragraph::new(output_text)
            .block(Block::default().borders(Borders::ALL).title("Git Output"))
            .wrap(Wrap { trim: false })
            .scroll((
                output.len().saturating_sub(main_chunks[1].height as usize) as u16,
                0,
            ));
        f.render_widget(output_widget, content_chunks[0]);

        // Right pane: Commit and PR details
        let mut details_text = vec![];

        if app.current_cherry_pick_index < app.cherry_pick_items.len() {
            let current_item = &app.cherry_pick_items[app.current_cherry_pick_index];

            // Shortened commit hash
            let short_hash = if current_item.commit_id.len() >= 8 {
                &current_item.commit_id[..8]
            } else {
                &current_item.commit_id
            };

            details_text.push(Line::from(vec![
                Span::raw("Hash: "),
                Span::styled(short_hash, Style::default().fg(Color::Yellow)),
            ]));

            details_text.push(Line::from(vec![
                Span::raw("PR #"),
                Span::styled(
                    format!("{}", current_item.pr_id),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            details_text.push(Line::from(""));
            details_text.push(Line::from(vec![
                Span::raw("Title: "),
                Span::raw(&current_item.pr_title),
            ]));

            if !self.conflicted_files.is_empty() {
                details_text.push(Line::from(""));
                details_text.push(Line::from(""));
                details_text.push(Line::from(vec![Span::styled(
                    "Previously conflicted files:",
                    Style::default().fg(Color::Gray),
                )]));
                for file in &self.conflicted_files {
                    details_text.push(Line::from(format!("  • {}", file)));
                }
            }
        }

        let details_widget = Paragraph::new(details_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Commit Details"),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(details_widget, content_chunks[1]);

        // Bottom: Instructions
        let instructions = if is_complete {
            match success {
                Some(true) => "Press any key to continue to next commit",
                Some(false) => "r: Retry | a: Abort cherry-pick process",
                None => "Press any key to continue",
            }
        } else {
            "Processing... Please wait (pre-commit hooks may take time)"
        };

        let instructions_widget = Paragraph::new(instructions)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .style(Style::default().fg(Color::White));
        f.render_widget(instructions_widget, main_chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        let is_complete = *self.is_complete.lock().unwrap();

        // Don't process keys until the command is complete
        if !is_complete {
            return StateChange::Keep;
        }

        let success = *self.success.lock().unwrap();

        match success {
            Some(true) => {
                // Success - mark as successful and continue to next commit
                app.cherry_pick_items[app.current_cherry_pick_index].status =
                    CherryPickStatus::Success;
                app.current_cherry_pick_index += 1;
                StateChange::Change(Box::new(CherryPickState::continue_after_conflict()))
            }
            Some(false) => {
                // Failed - allow retry or abort
                match code {
                    KeyCode::Char('r') => {
                        // Retry - go back to conflict resolution
                        StateChange::Change(Box::new(ConflictResolutionState::new(
                            self.conflicted_files.clone(),
                        )))
                    }
                    KeyCode::Char('a') => {
                        // Abort - mark as failed and continue
                        let error_msg = self
                            .error_message
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| "Unknown error".to_string());
                        app.cherry_pick_items[app.current_cherry_pick_index].status =
                            CherryPickStatus::Failed(error_msg);
                        app.current_cherry_pick_index += 1;
                        StateChange::Change(Box::new(CherryPickState::continue_after_conflict()))
                    }
                    _ => StateChange::Keep,
                }
            }
            None => {
                // Should not happen, but handle it
                StateChange::Keep
            }
        }
    }
}

impl CherryPickContinueState {
    #[cfg(test)]
    fn new_test(
        conflicted_files: Vec<String>,
        output: Vec<String>,
        is_complete: bool,
        success: Option<bool>,
        error_message: Option<String>,
    ) -> Self {
        Self {
            output: Arc::new(Mutex::new(output)),
            is_complete: Arc::new(Mutex::new(is_complete)),
            success: Arc::new(Mutex::new(success)),
            error_message: Arc::new(Mutex::new(error_message)),
            conflicted_files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{
            CherryPickItem, CherryPickStatus, CreatedBy, MergeCommit, PullRequestWithWorkItems,
        },
        ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_default},
        },
    };
    use insta::assert_snapshot;
    use std::path::PathBuf;

    /// # Cherry Pick Continue State - Processing
    ///
    /// Tests the cherry-pick continue screen while git command is running.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state in processing mode
    /// - Shows git output as it streams in
    /// - Displays commit details and status
    ///
    /// ## Expected Outcome
    /// - Should display "Processing" indicator
    /// - Should show git output in real-time
    /// - Should show waiting message
    #[test]
    fn test_cherry_pick_continue_processing() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            // Set up cherry-pick items
            harness.app.cherry_pick_items = vec![CherryPickItem {
                commit_id: "abc123def456".to_string(),
                pr_id: 100,
                pr_title: "Fix authentication vulnerability".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness.app.current_cherry_pick_index = 0;
            harness.app.repo_path = Some(PathBuf::from("/path/to/repo"));

            // Set up PR data for details display
            harness.app.pull_requests = vec![PullRequestWithWorkItems {
                pr: crate::models::PullRequest {
                    id: 100,
                    title: "Fix authentication vulnerability".to_string(),
                    closed_date: Some("2024-01-16T14:20:00Z".to_string()),
                    created_by: CreatedBy {
                        display_name: "John Doe".to_string(),
                    },
                    last_merge_commit: Some(MergeCommit {
                        commit_id: "abc123def456".to_string(),
                    }),
                    labels: None,
                },
                work_items: vec![],
                selected: false,
            }];

            // Create state with output showing git is processing
            let conflicted_files = vec![
                "src/auth/login.rs".to_string(),
                "src/auth/session.rs".to_string(),
            ];
            let output = vec![
                "Running pre-commit hooks...".to_string(),
                "Checking code formatting...".to_string(),
                "Running clippy...".to_string(),
            ];

            let state = Box::new(CherryPickContinueState::new_test(
                conflicted_files,
                output,
                false, // Not complete
                None,
                None,
            ));

            harness.render_state(state);
            assert_snapshot!("processing", harness.backend());
        });
    }

    /// # Cherry Pick Continue State - Success
    ///
    /// Tests the cherry-pick continue screen when commit succeeds.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state in success mode
    /// - Shows successful git output
    /// - Displays success indicator and instructions
    ///
    /// ## Expected Outcome
    /// - Should display green success indicator
    /// - Should show successful git output
    /// - Should prompt to continue to next commit
    #[test]
    fn test_cherry_pick_continue_success() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            // Set up cherry-pick items
            harness.app.cherry_pick_items = vec![CherryPickItem {
                commit_id: "abc123def456".to_string(),
                pr_id: 200,
                pr_title: "Add new feature for user management".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness.app.current_cherry_pick_index = 0;
            harness.app.repo_path = Some(PathBuf::from("/home/user/project"));

            // Set up PR data
            harness.app.pull_requests = vec![PullRequestWithWorkItems {
                pr: crate::models::PullRequest {
                    id: 200,
                    title: "Add new feature for user management".to_string(),
                    closed_date: Some("2024-02-11T16:45:00Z".to_string()),
                    created_by: CreatedBy {
                        display_name: "Jane Smith".to_string(),
                    },
                    last_merge_commit: Some(MergeCommit {
                        commit_id: "abc123def456".to_string(),
                    }),
                    labels: None,
                },
                work_items: vec![],
                selected: false,
            }];

            // Create state with successful output
            let conflicted_files = vec!["src/users/manager.rs".to_string()];
            let output = vec![
                "Running pre-commit hooks...".to_string(),
                "Checking code formatting... ✓".to_string(),
                "Running clippy... ✓".to_string(),
                "Running tests... ✓".to_string(),
                "[main abc1234] Add new feature for user management".to_string(),
                " 1 file changed, 45 insertions(+), 12 deletions(-)".to_string(),
            ];

            let state = Box::new(CherryPickContinueState::new_test(
                conflicted_files,
                output,
                true,       // Complete
                Some(true), // Success
                None,
            ));

            harness.render_state(state);
            assert_snapshot!("success", harness.backend());
        });
    }

    /// # Cherry Pick Continue State - Failure
    ///
    /// Tests the cherry-pick continue screen when commit fails.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state in failure mode
    /// - Shows error output from git
    /// - Displays retry and abort options
    ///
    /// ## Expected Outcome
    /// - Should display red failure indicator
    /// - Should show error output
    /// - Should offer retry (r) and abort (a) options
    #[test]
    fn test_cherry_pick_continue_failure() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            // Set up cherry-pick items
            harness.app.cherry_pick_items = vec![CherryPickItem {
                commit_id: "def456ghi789".to_string(),
                pr_id: 300,
                pr_title: "Update database schema for performance".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness.app.current_cherry_pick_index = 0;
            harness.app.repo_path = Some(PathBuf::from("/opt/project"));

            // Set up PR data
            harness.app.pull_requests = vec![PullRequestWithWorkItems {
                pr: crate::models::PullRequest {
                    id: 300,
                    title: "Update database schema for performance".to_string(),
                    closed_date: Some("2024-03-06T13:10:00Z".to_string()),
                    created_by: CreatedBy {
                        display_name: "Bob Wilson".to_string(),
                    },
                    last_merge_commit: Some(MergeCommit {
                        commit_id: "def456ghi789".to_string(),
                    }),
                    labels: None,
                },
                work_items: vec![],
                selected: false,
            }];

            // Create state with failure output
            let conflicted_files = vec![
                "src/db/schema.rs".to_string(),
                "migrations/001_initial.sql".to_string(),
            ];
            let output = vec![
                "Running pre-commit hooks...".to_string(),
                "Checking code formatting... ✓".to_string(),
                "Running clippy...".to_string(),
                "error: this function has too many arguments (9/7)".to_string(),
                "  --> src/db/schema.rs:42:1".to_string(),
                "   |".to_string(),
                "42 | pub fn update_schema(".to_string(),
                "   | ^^^^^^^^^^^^^^^^^^^^^".to_string(),
                "   |".to_string(),
                "error: could not compile `project` due to previous error".to_string(),
                "".to_string(),
                "Pre-commit hook failed. Fix the issues and retry.".to_string(),
            ];

            let error_message = Some(output.join("\n"));

            let state = Box::new(CherryPickContinueState::new_test(
                conflicted_files,
                output,
                true,        // Complete
                Some(false), // Failed
                error_message,
            ));

            harness.render_state(state);
            assert_snapshot!("failure", harness.backend());
        });
    }
}
