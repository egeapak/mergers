use super::MergeState;
use crate::{
    git,
    models::CherryPickStatus,
    ui::apps::MergeApp,
    ui::state::typed::{TypedModeState, TypedStateChange},
    ui::state::{CompletionState, ConflictResolutionState, ErrorState},
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

pub struct CherryPickState {
    processing: bool,
}

impl Default for CherryPickState {
    fn default() -> Self {
        Self::new()
    }
}

impl CherryPickState {
    pub fn new() -> Self {
        Self { processing: true }
    }

    pub fn continue_after_conflict() -> Self {
        Self { processing: false }
    }
}

// ============================================================================
// TypedModeState Implementation
// ============================================================================

#[async_trait]
impl TypedModeState for CherryPickState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area());

        let title = Paragraph::new("Cherry-picking Commits")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Split the main area horizontally
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);

        // Left side: Commit list
        let items: Vec<ListItem> = app
            .cherry_pick_items()
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let mut spans = vec![];

                let (symbol, color) = match &item.status {
                    CherryPickStatus::Pending => ("⏸", Color::Gray),
                    CherryPickStatus::InProgress => ("⏳", Color::Yellow),
                    CherryPickStatus::Success => ("✅", Color::Green),
                    CherryPickStatus::Conflict => ("⚠️", Color::Yellow),
                    CherryPickStatus::Skipped => ("⏭", Color::Gray),
                    CherryPickStatus::Failed(_) => ("❌", Color::Red),
                };

                spans.push(Span::styled(
                    format!("{} ", symbol),
                    Style::default().fg(color),
                ));
                spans.push(Span::raw(format!(
                    "[{}/{}] ",
                    i + 1,
                    app.cherry_pick_items().len()
                )));
                spans.push(Span::styled(
                    format!("PR #{}: ", item.pr_id),
                    Style::default().fg(Color::Cyan),
                ));

                // Truncate title if too long
                let title = if item.pr_title.len() > 40 {
                    format!("{}...", &item.pr_title[..37])
                } else {
                    item.pr_title.clone()
                };
                spans.push(Span::raw(title));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Commits"))
            .highlight_style(Style::default().bg(Color::DarkGray));
        f.render_widget(list, main_chunks[0]);

        // Right side: Details
        let mut details_text = vec![];

        if app.current_cherry_pick_index() < app.cherry_pick_items().len() {
            let current_item = &app.cherry_pick_items()[app.current_cherry_pick_index()];

            details_text.push(Line::from(vec![
                Span::raw("Current PR: "),
                Span::styled(
                    format!("#{}", current_item.pr_id),
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

            details_text.push(Line::from(""));
            details_text.push(Line::from(vec![
                Span::raw("Commit: "),
                Span::styled(
                    &current_item.commit_id[..8],
                    Style::default().fg(Color::Yellow),
                ),
            ]));

            details_text.push(Line::from(""));
            details_text.push(Line::from(vec![
                Span::raw("Status: "),
                Span::styled(
                    match &current_item.status {
                        CherryPickStatus::Pending => "Pending",
                        CherryPickStatus::InProgress => "In Progress",
                        CherryPickStatus::Success => "Success",
                        CherryPickStatus::Conflict => "Conflict",
                        CherryPickStatus::Skipped => "Skipped",
                        CherryPickStatus::Failed(_) => "Failed",
                    },
                    Style::default().fg(match &current_item.status {
                        CherryPickStatus::Success => Color::Green,
                        CherryPickStatus::Failed(_) => Color::Red,
                        CherryPickStatus::Conflict => Color::Yellow,
                        CherryPickStatus::InProgress => Color::Yellow,
                        CherryPickStatus::Skipped => Color::Gray,
                        _ => Color::White,
                    }),
                ),
            ]));

            if let CherryPickStatus::Failed(msg) = &current_item.status {
                details_text.push(Line::from(""));
                details_text.push(Line::from(vec![
                    Span::raw("Error: "),
                    Span::styled(msg, Style::default().fg(Color::Red)),
                ]));
            }
        }

        details_text.push(Line::from(""));
        details_text.push(Line::from("─────────────────────"));
        details_text.push(Line::from(""));

        let branch_name = format!(
            "patch/{}-{}",
            app.target_branch(),
            app.version().as_ref().unwrap()
        );

        details_text.push(Line::from(vec![
            Span::raw("Branch: "),
            Span::styled(branch_name, Style::default().fg(Color::Cyan)),
        ]));

        if let Some(repo_path) = &app.repo_path() {
            details_text.push(Line::from(vec![
                Span::raw("Location: "),
                Span::styled(
                    format!("{}", repo_path.display()),
                    Style::default().fg(Color::Blue),
                ),
            ]));
        }

        let details = Paragraph::new(details_text)
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .wrap(Wrap { trim: true });
        f.render_widget(details, main_chunks[1]);

        let status = if self.processing {
            "Processing cherry-picks..."
        } else {
            "Press any key to continue"
        };
        let status_widget = Paragraph::new(status)
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(status_widget, chunks[2]);
    }

    async fn process_key(
        &mut self,
        _code: KeyCode,
        app: &mut MergeApp,
    ) -> TypedStateChange<MergeState> {
        if self.processing {
            // First time processing - fetch commits if needed
            self.processing = false;

            let repo_path_opt = app.repo_path();
            let repo_path = repo_path_opt.as_ref().unwrap();

            // Fetch commits if needed (for cloned repositories)
            if app.local_repo().is_none() {
                let commits: Vec<String> = app
                    .cherry_pick_items()
                    .iter()
                    .map(|item| item.commit_id.clone())
                    .collect();

                if let Err(e) = git::fetch_commits(repo_path, &commits) {
                    app.set_error_message(Some(format!("Failed to fetch commits: {}", e)));
                    return TypedStateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
        }

        // Process next commit (either first time or continuing after conflict)
        process_next_commit(app)
    }

    fn name(&self) -> &'static str {
        "CherryPick"
    }
}

pub fn process_next_commit(app: &mut MergeApp) -> TypedStateChange<MergeState> {
    // Skip already processed commits
    while app.current_cherry_pick_index() < app.cherry_pick_items().len() {
        let item = &app.cherry_pick_items()[app.current_cherry_pick_index()];
        if matches!(item.status, CherryPickStatus::Pending) {
            break;
        }
        app.set_current_cherry_pick_index(app.current_cherry_pick_index() + 1);
    }

    // Check if we're done with all commits
    if app.current_cherry_pick_index() >= app.cherry_pick_items().len() {
        return TypedStateChange::Change(MergeState::Completion(CompletionState::new()));
    }

    // Process the current commit
    let current_index = app.current_cherry_pick_index();
    let repo_path = {
        let repo_path_ref = app.repo_path();
        repo_path_ref.unwrap().to_path_buf()
    };

    let item = &mut app.cherry_pick_items_mut()[current_index];
    item.status = CherryPickStatus::InProgress;
    let commit_id = item.commit_id.clone();

    match git::cherry_pick_commit(&repo_path, &commit_id) {
        Ok(git::CherryPickResult::Success) => {
            item.status = CherryPickStatus::Success;
            app.set_current_cherry_pick_index(app.current_cherry_pick_index() + 1);
            // Return to the same state to continue processing and show UI update
            TypedStateChange::Change(MergeState::CherryPick(
                CherryPickState::continue_after_conflict(),
            ))
        }
        Ok(git::CherryPickResult::Conflict(files)) => {
            item.status = CherryPickStatus::Conflict;
            TypedStateChange::Change(MergeState::ConflictResolution(
                ConflictResolutionState::new(files),
            ))
        }
        Ok(git::CherryPickResult::Failed(msg)) => {
            item.status = CherryPickStatus::Failed(msg);
            app.set_current_cherry_pick_index(app.current_cherry_pick_index() + 1);
            // Return to the same state to continue processing and show UI update
            TypedStateChange::Change(MergeState::CherryPick(
                CherryPickState::continue_after_conflict(),
            ))
        }
        Err(e) => {
            item.status = CherryPickStatus::Failed(e.to_string());
            app.set_current_cherry_pick_index(app.current_cherry_pick_index() + 1);
            // Return to the same state to continue processing and show UI update
            TypedStateChange::Change(MergeState::CherryPick(
                CherryPickState::continue_after_conflict(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{TuiTestHarness, create_test_cherry_pick_items, create_test_config_default},
    };
    use insta::assert_snapshot;
    use std::path::PathBuf;

    /// # Cherry Pick State - In Progress
    ///
    /// Tests the cherry-pick screen with mixed statuses.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick state
    /// - Sets up cherry-pick items with various statuses
    /// - Sets a version and repo path
    /// - Renders the cherry-pick progress display
    ///
    /// ## Expected Outcome
    /// - Should display commit list with status symbols
    /// - Should show current commit details
    /// - Should display progress counters
    #[test]
    fn test_cherry_pick_in_progress() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(1);

            let mut state = CherryPickState::new();
            harness.render_state(&mut state);

            assert_snapshot!("in_progress", harness.backend());
        });
    }

    /// # Cherry Pick State - With Conflict
    ///
    /// Tests the cherry-pick screen showing a conflict.
    ///
    /// ## Test Scenario
    /// - Creates cherry-pick items with a conflict
    /// - Sets current index to the conflicted item
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should highlight the conflict status
    /// - Should show conflict warning symbol
    #[test]
    fn test_cherry_pick_with_conflict() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(3); // Conflict item

            let mut state = CherryPickState::new();
            harness.render_state(&mut state);

            assert_snapshot!("with_conflict", harness.backend());
        });
    }

    /// # Cherry Pick State - With Failed Items
    ///
    /// Tests the cherry-pick screen showing failed items.
    ///
    /// ## Test Scenario
    /// - Creates cherry-pick items with some failed
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should show failed status with error message
    #[test]
    fn test_cherry_pick_with_failed() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut items = create_test_cherry_pick_items();
            items[0].status = CherryPickStatus::Success;
            items[1].status = CherryPickStatus::Failed("Unable to apply patch".to_string());
            *harness.app.cherry_pick_items_mut() = items;
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(2);

            let mut state = CherryPickState::new();
            harness.render_state(&mut state);

            assert_snapshot!("with_failed", harness.backend());
        });
    }

    /// # Cherry Pick State - With Skipped Items
    ///
    /// Tests the cherry-pick screen showing skipped items.
    ///
    /// ## Test Scenario
    /// - Creates cherry-pick items with some skipped
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should show skipped status indicator
    #[test]
    fn test_cherry_pick_with_skipped() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut items = create_test_cherry_pick_items();
            items[0].status = CherryPickStatus::Success;
            items[1].status = CherryPickStatus::Skipped;
            items[2].status = CherryPickStatus::Skipped;
            *harness.app.cherry_pick_items_mut() = items;
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(3);

            let mut state = CherryPickState::new();
            harness.render_state(&mut state);

            assert_snapshot!("with_skipped", harness.backend());
        });
    }

    /// # Cherry Pick State - All Success
    ///
    /// Tests the cherry-pick screen with all items successful.
    ///
    /// ## Test Scenario
    /// - Creates cherry-pick items all with success status
    /// - Renders the display at the end
    ///
    /// ## Expected Outcome
    /// - Should show all items as successful
    #[test]
    fn test_cherry_pick_all_success() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut items = create_test_cherry_pick_items();
            for item in &mut items {
                item.status = CherryPickStatus::Success;
            }
            *harness.app.cherry_pick_items_mut() = items;
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(4);

            let mut state = CherryPickState::new();
            harness.render_state(&mut state);

            assert_snapshot!("all_success", harness.backend());
        });
    }

    /// # CherryPickState Default Implementation
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates CherryPickState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should initialize with processing=true (same as new)
    #[test]
    fn test_cherry_pick_default() {
        let state = CherryPickState::default();
        assert!(state.processing);
    }

    /// # CherryPickState New Implementation
    ///
    /// Tests the new() method.
    ///
    /// ## Test Scenario
    /// - Creates CherryPickState using new()
    ///
    /// ## Expected Outcome
    /// - Should initialize with processing=true
    #[test]
    fn test_cherry_pick_new() {
        let state = CherryPickState::new();
        assert!(state.processing);
    }

    /// # CherryPickState Continue After Conflict
    ///
    /// Tests the continue_after_conflict() method.
    ///
    /// ## Test Scenario
    /// - Creates CherryPickState using continue_after_conflict()
    ///
    /// ## Expected Outcome
    /// - Should initialize with processing=false
    #[test]
    fn test_cherry_pick_continue_after_conflict() {
        let state = CherryPickState::continue_after_conflict();
        assert!(!state.processing);
    }

    /// # Cherry Pick State - Initial Index
    ///
    /// Tests rendering at initial index (0).
    ///
    /// ## Test Scenario
    /// - Creates cherry-pick state at index 0
    /// - First item is pending
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should show first item as pending/in progress
    #[test]
    fn test_cherry_pick_initial_index() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut items = create_test_cherry_pick_items();
            items[0].status = CherryPickStatus::Pending;
            *harness.app.cherry_pick_items_mut() = items;
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(0);

            let mut state = CherryPickState::new();
            harness.render_state(&mut state);

            assert_snapshot!("initial_index", harness.backend());
        });
    }

    /// # Cherry Pick State - Mixed Statuses End
    ///
    /// Tests rendering at the end with mixed statuses.
    ///
    /// ## Test Scenario
    /// - Creates cherry-pick items with mixed statuses
    /// - Index is at the end
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should show all items with their final status
    #[test]
    fn test_cherry_pick_mixed_end() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut items = create_test_cherry_pick_items();
            items[0].status = CherryPickStatus::Success;
            items[1].status = CherryPickStatus::Skipped;
            items[2].status = CherryPickStatus::Success;
            items[3].status = CherryPickStatus::Failed("Merge conflict".to_string());
            *harness.app.cherry_pick_items_mut() = items;
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));
            harness.app.set_current_cherry_pick_index(4);

            let mut state = CherryPickState::new();
            harness.render_state(&mut state);

            assert_snapshot!("mixed_end", harness.backend());
        });
    }
}
