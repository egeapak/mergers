use super::MergeState;
use crate::{
    core::state::{MergePhase, StateItemStatus},
    models::CherryPickStatus,
    ui::apps::MergeApp,
    ui::state::typed::{ModeState, StateChange},
    ui::state::{AbortingState, CherryPickState, ConflictResolutionState},
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
                .args(["cherry-pick", "--continue", "--no-edit"])
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

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for CherryPickContinueState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
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

        if app.current_cherry_pick_index() < app.cherry_pick_items().len() {
            let current_item = &app.cherry_pick_items()[app.current_cherry_pick_index()];

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

            // Display error message when cherry-pick fails
            if is_complete
                && success == Some(false)
                && let Some(ref error_msg) = *self.error_message.lock().unwrap()
            {
                details_text.push(Line::from(""));
                details_text.push(Line::from(""));
                details_text.push(Line::from(vec![Span::styled(
                    "Error:",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )]));
                // Show first few lines of error to avoid overwhelming the display
                for line in error_msg.lines().take(5) {
                    details_text.push(Line::from(vec![Span::styled(
                        format!("  {}", line),
                        Style::default().fg(Color::Red),
                    )]));
                }
                let line_count = error_msg.lines().count();
                if line_count > 5 {
                    details_text.push(Line::from(vec![Span::styled(
                        format!("  ... and {} more lines (see Git Output)", line_count - 5),
                        Style::default().fg(Color::DarkGray),
                    )]));
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
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let instructions_lines = if is_complete {
            match success {
                Some(true) => vec![Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("any key", key_style),
                    Span::raw(" to continue to next commit"),
                ])],
                Some(false) => vec![Line::from(vec![
                    Span::styled("r", key_style),
                    Span::raw(": Retry | "),
                    Span::styled("s", key_style),
                    Span::raw(": Skip commit | "),
                    Span::styled("a", key_style),
                    Span::raw(": Abort (cleanup)"),
                ])],
                None => vec![Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("any key", key_style),
                    Span::raw(" to continue"),
                ])],
            }
        } else {
            vec![Line::from(
                "Processing... Please wait (pre-commit hooks may take time)",
            )]
        };

        let instructions_widget = Paragraph::new(instructions_lines)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .style(Style::default().fg(Color::White));
        f.render_widget(instructions_widget, main_chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> StateChange<MergeState> {
        let is_complete = *self.is_complete.lock().unwrap();

        // Don't process keys until the command is complete
        if !is_complete {
            return StateChange::Keep;
        }

        let success = *self.success.lock().unwrap();

        match success {
            Some(true) => {
                // Success - mark as successful and continue to next commit
                let current_index = app.current_cherry_pick_index();
                app.cherry_pick_items_mut()[current_index].status = CherryPickStatus::Success;
                app.set_current_cherry_pick_index(current_index + 1);

                // Update state file with success status and resume cherry-picking phase
                let _ = app.update_state_item_status(current_index, StateItemStatus::Success);
                let _ = app.clear_state_conflicted_files();
                let _ = app.update_state_phase(MergePhase::CherryPicking);

                StateChange::Change(MergeState::CherryPick(
                    CherryPickState::continue_after_conflict(),
                ))
            }
            Some(false) => {
                // Failed - allow retry, skip, or abort
                match code {
                    KeyCode::Char('r') => {
                        // Retry - go back to conflict resolution
                        StateChange::Change(MergeState::ConflictResolution(
                            ConflictResolutionState::new(self.conflicted_files.clone()),
                        ))
                    }
                    KeyCode::Char('s') => {
                        // Skip - mark as skipped and continue to next commit
                        let current_index = app.current_cherry_pick_index();
                        app.cherry_pick_items_mut()[current_index].status =
                            CherryPickStatus::Skipped;
                        app.set_current_cherry_pick_index(current_index + 1);

                        // Update state file with skipped status and resume cherry-picking phase
                        let _ =
                            app.update_state_item_status(current_index, StateItemStatus::Skipped);
                        let _ = app.clear_state_conflicted_files();
                        let _ = app.update_state_phase(MergePhase::CherryPicking);

                        StateChange::Change(MergeState::CherryPick(
                            CherryPickState::continue_after_conflict(),
                        ))
                    }
                    KeyCode::Char('a') => {
                        // Abort entire process with cleanup - use AbortingState for immediate UI feedback
                        let repo_path_opt = app.repo_path();
                        let repo_path = repo_path_opt.as_ref().unwrap().to_path_buf();
                        let version_opt = app.version();
                        let version = version_opt.as_ref().unwrap().to_string();
                        let target_branch = app.target_branch().to_string();
                        let base_repo_path =
                            app.state_file().and_then(|sf| sf.base_repo_path.clone());
                        StateChange::Change(MergeState::Aborting(AbortingState::new(
                            base_repo_path,
                            repo_path,
                            version,
                            target_branch,
                        )))
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

    fn name(&self) -> &'static str {
        "CherryPickContinue"
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
            *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
                commit_id: "abc123def456".to_string(),
                pr_id: 100,
                pr_title: "Fix authentication vulnerability".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness.app.set_current_cherry_pick_index(0);
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));

            // Set up PR data for details display
            *harness.app.pull_requests_mut() = vec![PullRequestWithWorkItems {
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

            let mut state = CherryPickContinueState::new_test(
                conflicted_files,
                output,
                false, // Not complete
                None,
                None,
            );

            harness.render_state(&mut state);
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
            *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
                commit_id: "abc123def456".to_string(),
                pr_id: 200,
                pr_title: "Add new feature for user management".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness.app.set_current_cherry_pick_index(0);
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/home/user/project")));

            // Set up PR data
            *harness.app.pull_requests_mut() = vec![PullRequestWithWorkItems {
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

            let mut state = CherryPickContinueState::new_test(
                conflicted_files,
                output,
                true,       // Complete
                Some(true), // Success
                None,
            );

            harness.render_state(&mut state);
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
            *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
                commit_id: "def456ghi789".to_string(),
                pr_id: 300,
                pr_title: "Update database schema for performance".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness.app.set_current_cherry_pick_index(0);
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/opt/project")));

            // Set up PR data
            *harness.app.pull_requests_mut() = vec![PullRequestWithWorkItems {
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

            let mut state = CherryPickContinueState::new_test(
                conflicted_files,
                output,
                true,        // Complete
                Some(false), // Failed
                error_message,
            );

            harness.render_state(&mut state);
            assert_snapshot!("failure", harness.backend());
        });
    }

    /// # Cherry Pick Continue - Non-Interactive Mode Integration Test
    ///
    /// Tests that git cherry-pick --continue runs in non-interactive mode
    /// without prompting for a commit message.
    ///
    /// ## Test Scenario
    /// - Creates a git repository with conflicting changes
    /// - Triggers a cherry-pick conflict
    /// - Resolves the conflict and stages files
    /// - Runs the actual CherryPickContinueState which executes git cherry-pick --continue --no-edit
    ///
    /// ## Expected Outcome
    /// - The cherry-pick continue command should complete successfully
    /// - No editor should be opened for commit message input
    /// - The original commit message should be preserved
    #[test]
    fn test_cherry_pick_continue_non_interactive() {
        use std::process::Command;
        use tempfile::TempDir;

        // Set up test repository
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize git repo
        Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .unwrap();

        // Configure git user for commits
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        // Disable commit signing for test
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();

        // Set default branch to main
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "main"])
            .output()
            .unwrap();

        // Create initial commit with a file
        std::fs::write(repo_path.join("conflict.txt"), "original content").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create feature branch and modify the same file
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "feature"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("conflict.txt"), "feature content").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Feature commit message"])
            .output()
            .unwrap();

        // Get feature commit hash
        let output = Command::new("git")
            .current_dir(&repo_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let feature_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Go back to main and modify the same file differently to create conflict
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("conflict.txt"), "main content").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Main commit"])
            .output()
            .unwrap();

        // Start cherry-pick which will create a conflict
        let cherry_pick_result = Command::new("git")
            .current_dir(&repo_path)
            .args(["cherry-pick", &feature_hash])
            .output()
            .unwrap();

        // Verify cherry-pick failed due to conflict
        assert!(
            !cherry_pick_result.status.success(),
            "Expected cherry-pick to fail with conflict"
        );

        // Resolve the conflict by choosing the feature content
        std::fs::write(repo_path.join("conflict.txt"), "resolved content").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "conflict.txt"])
            .output()
            .unwrap();

        // Now create the CherryPickContinueState which will run git cherry-pick --continue --no-edit
        let conflicted_files = vec!["conflict.txt".to_string()];
        let state = CherryPickContinueState::new(conflicted_files, repo_path.clone());

        // Wait for the command to complete (with timeout)
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);

        while !*state.is_complete.lock().unwrap() {
            if start.elapsed() > timeout {
                panic!("Timed out waiting for cherry-pick continue to complete");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // Verify success
        let success = *state.success.lock().unwrap();
        assert_eq!(
            success,
            Some(true),
            "Cherry-pick continue should succeed without prompting for commit message"
        );

        // Verify the commit was created with the original message
        let log_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["log", "--oneline", "-1"])
            .output()
            .unwrap();
        let log_message = String::from_utf8_lossy(&log_output.stdout);
        assert!(
            log_message.contains("Feature commit message"),
            "Original commit message should be preserved, got: {}",
            log_message
        );
    }

    /// # Cherry Pick Continue - Skip After Failure
    ///
    /// Tests behavior when user presses 's' to skip after continue fails.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state that has failed
    /// - Simulates pressing 's' to skip
    ///
    /// ## Expected Outcome
    /// - Should mark the commit as Skipped
    /// - Should increment cherry_pick_index
    /// - Should return StateChange::Change to CherryPickState
    #[tokio::test]
    async fn test_cherry_pick_continue_skip_after_failure() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        harness
            .app
            .set_repo_path(Some(PathBuf::from("/path/to/repo")));
        *harness.app.cherry_pick_items_mut() = vec![
            CherryPickItem {
                commit_id: "abc123".to_string(),
                pr_id: 100,
                pr_title: "Test PR 1".to_string(),
                status: CherryPickStatus::Conflict,
            },
            CherryPickItem {
                commit_id: "def456".to_string(),
                pr_id: 101,
                pr_title: "Test PR 2".to_string(),
                status: CherryPickStatus::Pending,
            },
        ];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];
        let error_message = Some("Pre-commit hook failed".to_string());

        let mut state = CherryPickContinueState::new_test(
            conflicted_files,
            vec!["Error output".to_string()],
            true,        // Complete
            Some(false), // Failed
            error_message,
        );

        // Press 's' to skip
        let result =
            ModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut()).await;

        // Should transition to CherryPickState
        assert!(matches!(result, StateChange::Change(_)));

        // Should mark the commit as Skipped
        assert!(matches!(
            harness.app.cherry_pick_items()[0].status,
            CherryPickStatus::Skipped
        ));

        // Should increment index
        assert_eq!(harness.app.current_cherry_pick_index(), 1);
    }

    /// # Cherry Pick Continue - Abort After Failure
    ///
    /// Tests behavior when user presses 'a' to abort after continue fails.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state that has failed
    /// - Simulates pressing 'a' to abort with cleanup
    ///
    /// ## Expected Outcome
    /// - Should call cleanup_cherry_pick
    /// - Should return StateChange::Change to CompletionState
    #[tokio::test]
    async fn test_cherry_pick_continue_abort_after_failure() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        harness
            .app
            .set_repo_path(Some(PathBuf::from("/nonexistent/path")));
        harness.app.set_version(Some("v1.0.0".to_string()));
        *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 100,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Conflict,
        }];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];
        let error_message = Some("Pre-commit hook failed".to_string());

        let mut state = CherryPickContinueState::new_test(
            conflicted_files,
            vec!["Error output".to_string()],
            true,        // Complete
            Some(false), // Failed
            error_message,
        );

        // Press 'a' to abort
        let result =
            ModeState::process_key(&mut state, KeyCode::Char('a'), harness.merge_app_mut()).await;

        // Should transition to CompletionState
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Cherry Pick Continue - Skip Does Not Trigger When Successful
    ///
    /// Tests that 's' key does not trigger skip when the continue was successful.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state that succeeded
    /// - Simulates pressing 's'
    ///
    /// ## Expected Outcome
    /// - Should mark as Success (normal success path)
    /// - Should NOT mark as Skipped
    #[tokio::test]
    async fn test_cherry_pick_continue_skip_ignored_on_success() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        harness
            .app
            .set_repo_path(Some(PathBuf::from("/path/to/repo")));
        *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 100,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Conflict,
        }];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];

        let mut state = CherryPickContinueState::new_test(
            conflicted_files,
            vec!["Success output".to_string()],
            true,       // Complete
            Some(true), // Success
            None,
        );

        // Press 's' (any key continues on success)
        let result =
            ModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut()).await;

        // Should transition to CherryPickState
        assert!(matches!(result, StateChange::Change(_)));

        // Should mark as Success, NOT Skipped
        assert!(matches!(
            harness.app.cherry_pick_items()[0].status,
            CherryPickStatus::Success
        ));
    }

    /// # Cherry Pick Continue - Skip Preserves Other Commits
    ///
    /// Tests that skipping one commit doesn't affect other commits.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state with multiple commits
    /// - Simulates pressing 's' to skip after failure
    ///
    /// ## Expected Outcome
    /// - First commit marked as Skipped
    /// - Second commit still Pending
    #[tokio::test]
    async fn test_cherry_pick_continue_skip_preserves_other_commits() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        harness
            .app
            .set_repo_path(Some(PathBuf::from("/path/to/repo")));
        *harness.app.cherry_pick_items_mut() = vec![
            CherryPickItem {
                commit_id: "abc123".to_string(),
                pr_id: 100,
                pr_title: "Test PR 1".to_string(),
                status: CherryPickStatus::Conflict,
            },
            CherryPickItem {
                commit_id: "def456".to_string(),
                pr_id: 101,
                pr_title: "Test PR 2".to_string(),
                status: CherryPickStatus::Pending,
            },
            CherryPickItem {
                commit_id: "ghi789".to_string(),
                pr_id: 102,
                pr_title: "Test PR 3".to_string(),
                status: CherryPickStatus::Pending,
            },
        ];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];

        let mut state = CherryPickContinueState::new_test(
            conflicted_files,
            vec!["Error output".to_string()],
            true,        // Complete
            Some(false), // Failed
            Some("Hook error".to_string()),
        );

        // Press 's' to skip
        let result =
            ModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut()).await;

        assert!(matches!(result, StateChange::Change(_)));

        // First commit should be Skipped
        assert!(matches!(
            harness.app.cherry_pick_items()[0].status,
            CherryPickStatus::Skipped
        ));

        // Second and third commits should still be Pending
        assert!(matches!(
            harness.app.cherry_pick_items()[1].status,
            CherryPickStatus::Pending
        ));
        assert!(matches!(
            harness.app.cherry_pick_items()[2].status,
            CherryPickStatus::Pending
        ));

        // Index should be at second commit
        assert_eq!(harness.app.current_cherry_pick_index(), 1);
    }

    /// # Cherry Pick Continue - Retry Keeps Same Index
    ///
    /// Tests that retrying doesn't increment the index.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick continue state that has failed
    /// - Simulates pressing 'r' to retry
    ///
    /// ## Expected Outcome
    /// - Index stays the same
    /// - Returns ConflictResolutionState
    #[tokio::test]
    async fn test_cherry_pick_continue_retry_keeps_index() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        harness
            .app
            .set_repo_path(Some(PathBuf::from("/path/to/repo")));
        *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 100,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Conflict,
        }];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];

        let mut state = CherryPickContinueState::new_test(
            conflicted_files,
            vec!["Error output".to_string()],
            true,        // Complete
            Some(false), // Failed
            Some("Hook error".to_string()),
        );

        // Press 'r' to retry
        let result =
            ModeState::process_key(&mut state, KeyCode::Char('r'), harness.merge_app_mut()).await;

        // Should transition to ConflictResolutionState
        assert!(matches!(result, StateChange::Change(_)));

        // Index should still be 0
        assert_eq!(harness.app.current_cherry_pick_index(), 0);

        // Status should still be Conflict (not changed)
        assert!(matches!(
            harness.app.cherry_pick_items()[0].status,
            CherryPickStatus::Conflict
        ));
    }
}
