use super::MergeState;
use crate::{
    git,
    models::CherryPickStatus,
    ui::apps::MergeApp,
    ui::state::typed::{TypedModeState, TypedStateChange},
    ui::state::{CherryPickContinueState, CherryPickState},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub struct ConflictResolutionState {
    conflicted_files: Vec<String>,
}

impl ConflictResolutionState {
    pub fn new(conflicted_files: Vec<String>) -> Self {
        Self { conflicted_files }
    }

    fn render_commit_info(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        current_item: &crate::models::CherryPickItem,
        app: &MergeApp,
    ) {
        let mut commit_text = vec![];

        // Try to get detailed commit info from git
        if let Some(repo_path) = &app.repo_path() {
            match crate::git::get_commit_info(repo_path, &current_item.commit_id) {
                Ok(commit_info) => {
                    // Shortened commit hash
                    let short_hash = if commit_info.hash.len() >= 8 {
                        commit_info.hash[..8].to_string()
                    } else {
                        commit_info.hash.clone()
                    };

                    commit_text.push(Line::from(vec![
                        Span::raw("Hash: "),
                        Span::styled(short_hash, Style::default().fg(Color::Yellow)),
                    ]));

                    // Format and display date
                    let date_part = if let Some(space_pos) = commit_info.date.find(' ') {
                        commit_info.date[..space_pos].to_string()
                    } else {
                        commit_info.date.clone()
                    };
                    commit_text.push(Line::from(vec![
                        Span::raw("Date: "),
                        Span::styled(date_part, Style::default().fg(Color::Gray)),
                    ]));

                    commit_text.push(Line::from(vec![
                        Span::raw("Author: "),
                        Span::styled(commit_info.author, Style::default().fg(Color::Green)),
                    ]));

                    commit_text.push(Line::from(""));
                    commit_text.push(Line::from(vec![
                        Span::raw("Title: "),
                        Span::raw(commit_info.title),
                    ]));
                }
                Err(_) => {
                    // Fallback to basic info if git command fails
                    let short_hash = if current_item.commit_id.len() >= 8 {
                        &current_item.commit_id[..8]
                    } else {
                        &current_item.commit_id
                    };

                    commit_text.push(Line::from(vec![
                        Span::raw("Hash: "),
                        Span::styled(short_hash, Style::default().fg(Color::Yellow)),
                    ]));

                    commit_text.push(Line::from(vec![
                        Span::raw("Title: "),
                        Span::raw(&current_item.pr_title),
                    ]));
                }
            }
        } else {
            // Fallback if no repo path available
            let short_hash = if current_item.commit_id.len() >= 8 {
                &current_item.commit_id[..8]
            } else {
                &current_item.commit_id
            };

            commit_text.push(Line::from(vec![
                Span::raw("Hash: "),
                Span::styled(short_hash, Style::default().fg(Color::Yellow)),
            ]));

            commit_text.push(Line::from(vec![
                Span::raw("Title: "),
                Span::raw(&current_item.pr_title),
            ]));
        }

        let commit_widget = Paragraph::new(commit_text)
            .block(Block::default().borders(Borders::ALL).title("Commit"))
            .wrap(Wrap { trim: true });
        f.render_widget(commit_widget, area);
    }

    fn render_conflicted_files(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let files: Vec<ListItem> = self
            .conflicted_files
            .iter()
            .map(|file| ListItem::new(format!("  • {}", file)))
            .collect();

        let file_list = List::new(files)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Conflicted Files"),
            )
            .style(Style::default().fg(Color::Red));
        f.render_widget(file_list, area);
    }

    fn render_pr_details(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        pr: Option<&crate::models::PullRequest>,
    ) {
        let mut pr_text = vec![];

        if let Some(pr) = pr {
            pr_text.push(Line::from(vec![
                Span::raw("PR #"),
                Span::styled(
                    format!("{}", pr.id),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            if let Some(date) = &pr.closed_date {
                pr_text.push(Line::from(vec![
                    Span::raw("Date: "),
                    Span::styled(date, Style::default().fg(Color::Gray)),
                ]));
            }

            pr_text.push(Line::from(vec![
                Span::raw("Author: "),
                Span::styled(
                    &pr.created_by.display_name,
                    Style::default().fg(Color::Green),
                ),
            ]));

            pr_text.push(Line::from(""));
            pr_text.push(Line::from(vec![Span::raw("Title: "), Span::raw(&pr.title)]));
        } else {
            pr_text.push(Line::from("PR details not found"));
        }

        let pr_widget = Paragraph::new(pr_text)
            .block(Block::default().borders(Borders::ALL).title("Pull Request"))
            .wrap(Wrap { trim: true });
        f.render_widget(pr_widget, area);
    }

    fn render_work_item_details(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        work_items: &[crate::models::WorkItem],
    ) {
        let mut wi_text = vec![];

        if work_items.is_empty() {
            wi_text.push(Line::from("No work items linked"));
        } else {
            for (i, wi) in work_items.iter().enumerate() {
                if i > 0 {
                    wi_text.push(Line::from(""));
                    wi_text.push(Line::from("─────────────────"));
                    wi_text.push(Line::from(""));
                }

                wi_text.push(Line::from(vec![
                    Span::styled(
                        wi.fields.work_item_type.as_deref().unwrap_or("Item"),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" #"),
                    Span::styled(format!("{}", wi.id), Style::default().fg(Color::Cyan)),
                ]));

                if let Some(state) = &wi.fields.state {
                    wi_text.push(Line::from(vec![
                        Span::raw("State: "),
                        Span::styled(state, Style::default().fg(Color::Yellow)),
                    ]));
                }

                if let Some(assigned_to) = &wi.fields.assigned_to {
                    wi_text.push(Line::from(vec![
                        Span::raw("Assigned: "),
                        Span::styled(&assigned_to.display_name, Style::default().fg(Color::Green)),
                    ]));
                }

                if let Some(title) = &wi.fields.title {
                    wi_text.push(Line::from(""));
                    wi_text.push(Line::from(vec![Span::raw("Title: "), Span::raw(title)]));
                }
            }
        }

        let wi_widget = Paragraph::new(wi_text)
            .block(Block::default().borders(Borders::ALL).title("Work Items"))
            .wrap(Wrap { trim: true });
        f.render_widget(wi_widget, area);
    }
}

// ============================================================================
// TypedModeState Implementation
// ============================================================================

#[async_trait]
impl TypedModeState for ConflictResolutionState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        // Main layout: Title at top, content in middle, help at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(0),    // Content
                Constraint::Length(5), // Instructions + Help
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("⚠️  Merge Conflict Detected")
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Split content horizontally: Left and Right panes
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);

        // Left pane: Split vertically for commit info (20%) and conflicted files (80%)
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(content_chunks[0]);

        // Right pane: Split vertically for PR details and work item details
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(content_chunks[1]);

        // Get current cherry-pick item
        let current_item = &app.cherry_pick_items()[app.current_cherry_pick_index()];

        // Find the corresponding PR and work items
        let pr_with_work_items = app
            .pull_requests()
            .iter()
            .find(|pr| pr.pr.id == current_item.pr_id);

        // Top-left: Commit Information (20%)
        self.render_commit_info(f, left_chunks[0], current_item, app);

        // Bottom-left: Conflicted Files (80%)
        self.render_conflicted_files(f, left_chunks[1]);

        // Top-right: PR Details
        self.render_pr_details(f, right_chunks[0], pr_with_work_items.map(|p| &p.pr));

        // Bottom-right: Work Item Details
        let work_items = pr_with_work_items
            .map(|p| p.work_items.as_slice())
            .unwrap_or(&[]);
        self.render_work_item_details(f, right_chunks[1], work_items);

        // Bottom: Instructions and Help
        let repo_path = app.repo_path().as_ref().unwrap().display();
        let instructions = vec![
            Line::from(vec![
                Span::raw("Repository: "),
                Span::styled(format!("{}", repo_path), Style::default().fg(Color::Cyan)),
            ]),
            Line::from("Please resolve conflicts in another terminal and stage the changes."),
            Line::from(vec![Span::styled(
                "c: Continue (after resolving) | s: Skip commit | a: Abort (cleanup)",
                Style::default().fg(Color::Gray),
            )]),
        ];

        let instructions_widget = Paragraph::new(instructions)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .style(Style::default().fg(Color::White));
        f.render_widget(instructions_widget, main_chunks[2]);
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MergeApp,
    ) -> TypedStateChange<MergeState> {
        let repo_path = {
            let repo_path_ref = app.repo_path();
            repo_path_ref.unwrap().to_path_buf()
        };

        match code {
            KeyCode::Char('c') => {
                // Check if conflicts are resolved
                match git::check_conflicts_resolved(&repo_path) {
                    Ok(true) => {
                        // Transition to CherryPickContinueState to process the commit with feedback
                        TypedStateChange::Change(MergeState::CherryPickContinue(
                            CherryPickContinueState::new(
                                self.conflicted_files.clone(),
                                repo_path.clone(),
                            ),
                        ))
                    }
                    Ok(false) => TypedStateChange::Keep, // Conflicts not resolved
                    Err(_) => TypedStateChange::Keep,
                }
            }
            KeyCode::Char('s') => {
                // Skip current commit - abort cherry-pick, mark as skipped, continue
                let _ = git::abort_cherry_pick(&repo_path);
                let current_index = app.current_cherry_pick_index();
                app.cherry_pick_items_mut()[current_index].status = CherryPickStatus::Skipped;
                app.set_current_cherry_pick_index(current_index + 1);
                TypedStateChange::Change(MergeState::CherryPick(
                    CherryPickState::continue_after_conflict(),
                ))
            }
            KeyCode::Char('a') => {
                // Abort entire process with cleanup
                let version_opt = app.version();
                let version = version_opt.as_ref().unwrap();
                let target_branch = app.target_branch().to_string();
                let _ = git::cleanup_cherry_pick(
                    None, // base_repo_path is no longer stored in App
                    &repo_path,
                    version,
                    &target_branch,
                );
                TypedStateChange::Change(MergeState::Completion(super::CompletionState::new()))
            }
            _ => TypedStateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "ConflictResolution"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{CherryPickItem, CherryPickStatus},
        ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_cherry_pick_items, create_test_config_default},
        },
    };
    use insta::assert_snapshot;
    use std::path::PathBuf;

    /// # Conflict Resolution State - Display
    ///
    /// Tests the conflict resolution screen.
    ///
    /// ## Test Scenario
    /// - Creates a conflict resolution state
    /// - Sets up a conflicted PR
    /// - Renders the conflict resolution screen
    ///
    /// ## Expected Outcome
    /// - Should display conflict warning
    /// - Should show conflicted PR details
    /// - Should display resolution options
    #[test]
    fn test_conflict_resolution_display() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
                commit_id: "abc123".to_string(),
                pr_id: 100,
                pr_title: "Fix critical database migration bug".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(0);

            let conflicted_files = vec!["src/database/migrations.rs".to_string()];
            let mut state = ConflictResolutionState::new(conflicted_files);
            harness.render_state(&mut state);

            assert_snapshot!("conflict_display", harness.backend());
        });
    }

    /// # Conflict Resolution - Continue with Unresolved Conflicts
    ///
    /// Tests behavior when user presses 'c' but conflicts are not resolved.
    ///
    /// ## Test Scenario
    /// - Creates a conflict resolution state
    /// - Sets up app with a non-existent repo path (will fail git check)
    /// - Simulates pressing 'c' to continue
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep (stay in same state)
    /// - This exercises the error path when git::check_conflicts_resolved fails
    #[tokio::test]
    async fn test_conflict_resolution_continue_unresolved() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        // Set up with non-existent repo to trigger error path
        harness
            .app
            .set_repo_path(Some(PathBuf::from("/nonexistent/repo/path")));
        *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 100,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Conflict,
        }];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];
        let mut state = ConflictResolutionState::new(conflicted_files);

        // Press 'c' to attempt continue
        let result =
            TypedModeState::process_key(&mut state, KeyCode::Char('c'), harness.merge_app_mut())
                .await;

        // Should stay in same state because git operation fails
        assert!(matches!(result, TypedStateChange::Keep));
    }

    /// # Conflict Resolution - Abort Cherry-Pick
    ///
    /// Tests behavior when user presses 'a' to abort.
    ///
    /// ## Test Scenario
    /// - Creates a conflict resolution state
    /// - Simulates pressing 'a' to abort
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to CompletionState
    /// - This exercises the abort path
    #[tokio::test]
    async fn test_conflict_resolution_abort() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        // Set up with non-existent repo (abort will fail silently which is OK)
        harness
            .app
            .set_repo_path(Some(PathBuf::from("/nonexistent/repo/path")));
        harness.app.set_version(Some("v1.0.0".to_string())); // Required for cleanup
        *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 100,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Conflict,
        }];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];
        let mut state = ConflictResolutionState::new(conflicted_files);

        // Press 'a' to abort
        let result =
            TypedModeState::process_key(&mut state, KeyCode::Char('a'), harness.merge_app_mut())
                .await;

        // Should transition to CompletionState
        assert!(matches!(result, TypedStateChange::Change(_)));
    }

    /// # Conflict Resolution - Other Key Press
    ///
    /// Tests behavior when user presses other keys.
    ///
    /// ## Test Scenario
    /// - Creates a conflict resolution state
    /// - Simulates pressing various other keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep for all other keys
    #[tokio::test]
    async fn test_conflict_resolution_other_keys() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        harness
            .app
            .set_repo_path(Some(PathBuf::from("/nonexistent/repo/path")));
        *harness.app.cherry_pick_items_mut() = vec![CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 100,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Conflict,
        }];
        harness.app.set_current_cherry_pick_index(0);

        let conflicted_files = vec!["test.rs".to_string()];
        let mut state = ConflictResolutionState::new(conflicted_files);

        // Test various keys that should be ignored
        for key in [
            KeyCode::Char('x'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Up,
        ] {
            let result =
                TypedModeState::process_key(&mut state, key, harness.merge_app_mut()).await;
            assert!(matches!(result, TypedStateChange::Keep));
        }
    }

    /// # Conflict Resolution - Skip Commit
    ///
    /// Tests behavior when user presses 's' to skip current commit.
    ///
    /// ## Test Scenario
    /// - Creates a conflict resolution state
    /// - Simulates pressing 's' to skip
    ///
    /// ## Expected Outcome
    /// - Should mark the commit as Skipped
    /// - Should increment cherry_pick_index
    /// - Should return StateChange::Change to CherryPickState
    #[tokio::test]
    async fn test_conflict_resolution_skip() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        // Set up with non-existent repo (abort will fail silently which is OK)
        harness
            .app
            .set_repo_path(Some(PathBuf::from("/nonexistent/repo/path")));
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
        let mut state = ConflictResolutionState::new(conflicted_files);

        // Press 's' to skip
        let result =
            TypedModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut())
                .await;

        // Should transition to CherryPickState
        assert!(matches!(result, TypedStateChange::Change(_)));

        // Should mark the commit as Skipped
        assert!(matches!(
            harness.app.cherry_pick_items()[0].status,
            CherryPickStatus::Skipped
        ));

        // Should increment index
        assert_eq!(harness.app.current_cherry_pick_index(), 1);
    }

    /// # Conflict Resolution - Skip Preserves Second Commit Status
    ///
    /// Tests that skipping one commit doesn't affect other commits.
    ///
    /// ## Test Scenario
    /// - Creates a conflict resolution state with multiple commits
    /// - Simulates pressing 's' to skip
    ///
    /// ## Expected Outcome
    /// - First commit marked as Skipped
    /// - Second commit still Pending
    #[tokio::test]
    async fn test_conflict_resolution_skip_preserves_other_commits() {
        use crossterm::event::KeyCode;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        harness
            .app
            .set_repo_path(Some(PathBuf::from("/nonexistent/repo/path")));
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
        let mut state = ConflictResolutionState::new(conflicted_files);

        // Press 's' to skip
        let result =
            TypedModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut())
                .await;

        assert!(matches!(result, TypedStateChange::Change(_)));

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

    /// # ConflictResolutionState Default Implementation
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates ConflictResolutionState using new() with empty files
    ///
    /// ## Expected Outcome
    /// - Should initialize with empty conflicted files
    #[test]
    fn test_conflict_resolution_default_new() {
        let state = ConflictResolutionState::new(vec![]);
        assert!(state.conflicted_files.is_empty());
    }

    /// # Conflict Resolution - Multiple Files Display
    ///
    /// Tests display with many conflicted files.
    ///
    /// ## Test Scenario
    /// - Creates state with multiple conflicted files
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display all conflicted files
    #[test]
    fn test_conflict_resolution_multiple_files() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
            harness.app.cherry_pick_items_mut()[0].status = CherryPickStatus::Conflict;
            harness.app.set_current_cherry_pick_index(0);

            let conflicted_files = vec![
                "src/main.rs".to_string(),
                "src/lib.rs".to_string(),
                "Cargo.toml".to_string(),
                "README.md".to_string(),
            ];
            let mut state = ConflictResolutionState::new(conflicted_files);
            harness.render_state(&mut state);

            assert_snapshot!("multiple_files", harness.backend());
        });
    }
}
