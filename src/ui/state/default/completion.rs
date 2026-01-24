use crate::{
    core::state::MergeStatus,
    models::CherryPickStatus,
    ui::apps::MergeApp,
    ui::state::default::MergeState,
    ui::state::typed::{ModeState, StateChange},
    utils::truncate_str,
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

pub struct CompletionState {
    list_state: ListState,
}

impl Default for CompletionState {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletionState {
    pub fn new() -> Self {
        let mut state = Self {
            list_state: ListState::default(),
        };
        state.list_state.select(Some(0));
        state
    }

    fn next(&mut self, app: &MergeApp) {
        if app.cherry_pick_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= app.cherry_pick_items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous(&mut self, app: &MergeApp) {
        if app.cherry_pick_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    app.cherry_pick_items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }
}

#[async_trait]
impl ModeState for CompletionState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(f.area());

        let title = Paragraph::new("🏁 Cherry-pick Process Completed!")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Split the main area horizontally
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(main_chunks[1]);

        // Left side: Commit status list
        let available_width = content_chunks[0].width.saturating_sub(4); // Account for borders

        let items: Vec<ListItem> = app
            .cherry_pick_items
            .iter()
            .map(|item| {
                let mut spans = vec![];

                let (symbol, color) = match &item.status {
                    CherryPickStatus::Success => ("✅", Color::Green),
                    CherryPickStatus::Failed(_) => ("❌", Color::Red),
                    CherryPickStatus::Conflict => ("⚠️", Color::Yellow),
                    CherryPickStatus::Skipped => ("⏭", Color::Gray),
                    _ => ("❓", Color::White),
                };

                spans.push(Span::styled(
                    format!("{} ", symbol),
                    Style::default().fg(color),
                ));

                let pr_prefix = format!("PR #{}: ", item.pr_id);
                spans.push(Span::styled(
                    pr_prefix.clone(),
                    Style::default().fg(Color::Cyan),
                ));

                // Find work items for this PR
                let work_items: Vec<i32> = app
                    .pull_requests
                    .iter()
                    .find(|pr| pr.pr.id == item.pr_id)
                    .map(|pr| pr.work_items.iter().map(|wi| wi.id).collect())
                    .unwrap_or_default();

                let work_items_text = if work_items.is_empty() {
                    String::new()
                } else {
                    format!(
                        " [WI: {}]",
                        work_items
                            .iter()
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };

                // Calculate available space for title
                let used_space = 3 + pr_prefix.len() + work_items_text.len(); // symbol + space + pr_prefix + work_items
                let title_space = if available_width as usize > used_space {
                    available_width as usize - used_space
                } else {
                    20 // minimum space
                };

                // Truncate title if needed to fit available space
                let title = if item.pr_title.len() > title_space {
                    if title_space > 3 {
                        format!(
                            "{}...",
                            truncate_str(&item.pr_title, title_space.saturating_sub(3))
                        )
                    } else {
                        "...".to_string()
                    }
                } else {
                    item.pr_title.clone()
                };
                spans.push(Span::raw(title));

                if !work_items.is_empty() {
                    spans.push(Span::styled(
                        work_items_text,
                        Style::default().fg(Color::Magenta),
                    ));
                }

                if let CherryPickStatus::Failed(msg) = &item.status {
                    let max_error_len = (available_width as usize)
                        .saturating_sub(used_space + item.pr_title.len() + 3);
                    let error_text = if msg.len() > max_error_len && max_error_len > 3 {
                        format!(
                            " - {}...",
                            truncate_str(msg, max_error_len.saturating_sub(6))
                        )
                    } else if max_error_len > 0 {
                        format!(" - {}", msg)
                    } else {
                        String::new()
                    };
                    if !error_text.is_empty() {
                        spans.push(Span::styled(error_text, Style::default().fg(Color::Red)));
                    }
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Cherry-pick Results"),
            )
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .fg(Color::Yellow),
            );
        f.render_stateful_widget(list, content_chunks[0], &mut self.list_state);

        // Right side: Summary and info
        let mut summary_text = vec![];

        // Calculate summary
        let mut successful = 0;
        let mut failed = 0;
        for item in &app.cherry_pick_items {
            match &item.status {
                CherryPickStatus::Success => successful += 1,
                CherryPickStatus::Failed(_) => failed += 1,
                _ => {}
            }
        }

        summary_text.push(Line::from(vec![Span::styled(
            "Summary",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        summary_text.push(Line::from(""));
        summary_text.push(Line::from(vec![
            Span::raw("✅ Successful: "),
            Span::styled(
                format!("{}", successful),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        summary_text.push(Line::from(vec![
            Span::raw("❌ Failed: "),
            Span::styled(
                format!("{}", failed),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]));

        summary_text.push(Line::from(""));
        summary_text.push(Line::from("─────────────────────"));
        summary_text.push(Line::from(""));

        summary_text.push(Line::from(vec![Span::styled(
            "Branch Info",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        summary_text.push(Line::from(""));

        let branch_name = format!(
            "patch/{}-{}",
            app.target_branch(),
            app.version.as_ref().unwrap()
        );
        summary_text.push(Line::from(vec![
            Span::raw("Branch: "),
            Span::styled(branch_name, Style::default().fg(Color::Cyan)),
        ]));

        if let Some(repo_path) = app.repo_path() {
            summary_text.push(Line::from(vec![
                Span::raw("Location: "),
                Span::styled(
                    format!("{}", repo_path.display()),
                    Style::default().fg(Color::Blue),
                ),
            ]));
        }

        summary_text.push(Line::from(""));
        summary_text.push(Line::from("─────────────────────"));
        summary_text.push(Line::from(""));

        summary_text.push(Line::from(vec![Span::styled(
            "Actions",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        summary_text.push(Line::from(""));
        summary_text.push(Line::from("↑/↓ Navigate"));
        summary_text.push(Line::from("'p' Open PR in browser"));
        summary_text.push(Line::from("'w' Open work items"));
        summary_text.push(Line::from(format!(
            "'t' Tag PRs & update work items to '{}'",
            app.work_item_state()
        )));
        summary_text.push(Line::from("'q' Exit"));

        let summary = Paragraph::new(summary_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Summary & Info"),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(summary, content_chunks[1]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> StateChange<MergeState> {
        match code {
            KeyCode::Char('q') => {
                // Mark state file as completed and clean up before exit
                if let Some(state_file) = app.state_file_mut() {
                    state_file.final_status = Some(MergeStatus::Success);
                    state_file.completed_at = Some(chrono::Utc::now());
                    let _ = state_file.save_for_repo();
                }
                let _ = app.cleanup_state_file();
                StateChange::Exit
            }
            KeyCode::Up => {
                self.previous(app);
                StateChange::Keep
            }
            KeyCode::Down => {
                self.next(app);
                StateChange::Keep
            }
            KeyCode::Char('p') => {
                if let Some(i) = self.list_state.selected()
                    && let Some(item) = app.cherry_pick_items.get(i)
                {
                    app.open_pr_in_browser(item.pr_id);
                }
                StateChange::Keep
            }
            KeyCode::Char('w') => {
                if let Some(i) = self.list_state.selected()
                    && let Some(item) = app.cherry_pick_items.get(i)
                {
                    // Find the corresponding PR and open its work items
                    if let Some(pr) = app.pull_requests.iter().find(|pr| pr.pr.id == item.pr_id)
                        && !pr.work_items.is_empty()
                    {
                        app.open_work_items_in_browser(&pr.work_items);
                    }
                }
                StateChange::Keep
            }
            KeyCode::Char('t') => StateChange::Change(MergeState::PostCompletion(
                crate::ui::state::PostCompletionState::new(),
            )),
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "Completion"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::CherryPickStatus,
        ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_cherry_pick_items, create_test_config_default},
        },
    };
    use insta::assert_snapshot;
    use std::path::PathBuf;

    /// # Completion State - Success
    ///
    /// Tests the completion screen with all successful cherry-picks.
    ///
    /// ## Test Scenario
    /// - Creates a completion state
    /// - Sets all cherry-pick items to success status
    /// - Renders the completion summary
    ///
    /// ## Expected Outcome
    /// - Should display success message
    /// - Should show summary of all successful items
    /// - Should display next steps options
    #[test]
    fn test_completion_success() {
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

            let mut state = CompletionState::new();
            harness.render_state(&mut state);

            assert_snapshot!("success", harness.backend());
        });
    }

    /// # Completion State - With Conflicts
    ///
    /// Tests the completion screen with some conflicts.
    ///
    /// ## Test Scenario
    /// - Creates a completion state
    /// - Sets some items to conflict status
    /// - Renders the completion summary
    ///
    /// ## Expected Outcome
    /// - Should show mixed results
    /// - Should highlight conflicted items
    #[test]
    fn test_completion_with_conflicts() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));

            let mut state = CompletionState::new();
            harness.render_state(&mut state);

            assert_snapshot!("with_conflicts", harness.backend());
        });
    }

    /// # Completion State - Navigate Down
    ///
    /// Tests down arrow navigation.
    ///
    /// ## Test Scenario
    /// - Creates completion state with multiple items
    /// - Processes down arrow key
    ///
    /// ## Expected Outcome
    /// - Selection should move to next item
    #[tokio::test]
    async fn test_completion_navigate_down() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();
        assert_eq!(state.list_state.selected(), Some(0));

        let result =
            ModeState::process_key(&mut state, KeyCode::Down, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert_eq!(state.list_state.selected(), Some(1));
    }

    /// # Completion State - Navigate Up
    ///
    /// Tests up arrow navigation.
    ///
    /// ## Test Scenario
    /// - Creates completion state
    /// - Processes up arrow key (should wrap to end)
    ///
    /// ## Expected Outcome
    /// - Selection should wrap to last item
    #[tokio::test]
    async fn test_completion_navigate_up() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();
        assert_eq!(state.list_state.selected(), Some(0));

        let result = ModeState::process_key(&mut state, KeyCode::Up, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        // Should wrap to last item
        assert_eq!(
            state.list_state.selected(),
            Some(harness.app.cherry_pick_items().len() - 1)
        );
    }

    /// # Completion State - Quit Key
    ///
    /// Tests 'q' key to exit.
    ///
    /// ## Test Scenario
    /// - Processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_completion_quit() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Completion State - Tag PRs Key
    ///
    /// Tests 't' key to proceed to tagging.
    ///
    /// ## Test Scenario
    /// - Processes 't' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to PostCompletionState
    #[tokio::test]
    async fn test_completion_tag_prs() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('t'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Completion State - Open PR Key
    ///
    /// Tests 'p' key to open PR in browser.
    ///
    /// ## Test Scenario
    /// - Processes 'p' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_completion_open_pr() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('p'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # Completion State - Open Work Items Key
    ///
    /// Tests 'w' key to open work items in browser.
    ///
    /// ## Test Scenario
    /// - Processes 'w' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_completion_open_work_items() {
        use crate::ui::testing::create_test_pull_requests;

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        *harness.app.pull_requests_mut() = create_test_pull_requests();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('w'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # Completion State - Other Keys Ignored
    ///
    /// Tests that other keys are ignored.
    ///
    /// ## Test Scenario
    /// - Processes various unrecognized keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_completion_other_keys() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();

        for key in [KeyCode::Char('x'), KeyCode::Esc, KeyCode::Enter] {
            let result = ModeState::process_key(&mut state, key, harness.merge_app_mut()).await;
            assert!(matches!(result, StateChange::Keep));
        }
    }

    /// # CompletionState Default Implementation
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates CompletionState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should have first item selected
    #[test]
    fn test_completion_default() {
        let state = CompletionState::default();
        assert_eq!(state.list_state.selected(), Some(0));
    }

    /// # Completion State - Empty Items Navigation
    ///
    /// Tests navigation with empty cherry-pick items.
    ///
    /// ## Test Scenario
    /// - Creates completion state with no items
    /// - Tries to navigate
    ///
    /// ## Expected Outcome
    /// - Should handle empty list gracefully
    #[tokio::test]
    async fn test_completion_empty_items_navigation() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        // Leave cherry_pick_items empty
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();

        // Should not panic
        let result =
            ModeState::process_key(&mut state, KeyCode::Down, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));

        let result = ModeState::process_key(&mut state, KeyCode::Up, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # Completion State - With Skipped Items
    ///
    /// Tests the completion screen with skipped items.
    ///
    /// ## Test Scenario
    /// - Creates a completion state
    /// - Sets some items to skipped status
    /// - Renders the completion summary
    ///
    /// ## Expected Outcome
    /// - Should show skipped items with appropriate indicator
    #[test]
    fn test_completion_with_skipped() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut items = create_test_cherry_pick_items();
            items[0].status = CherryPickStatus::Success;
            items[1].status = CherryPickStatus::Skipped;
            items[2].status = CherryPickStatus::Success;
            items[3].status = CherryPickStatus::Skipped;
            *harness.app.cherry_pick_items_mut() = items;
            harness.app.set_version(Some("v1.0.0".to_string()));
            harness
                .app
                .set_repo_path(Some(PathBuf::from("/path/to/repo")));

            let mut state = CompletionState::new();
            harness.render_state(&mut state);

            assert_snapshot!("with_skipped", harness.backend());
        });
    }

    /// # Completion State - Navigation Wrapping
    ///
    /// Tests that navigation wraps correctly at boundaries.
    ///
    /// ## Test Scenario
    /// - Creates completion state
    /// - Navigates past the end
    /// - Navigates before the beginning
    ///
    /// ## Expected Outcome
    /// - Should wrap around correctly
    #[tokio::test]
    async fn test_completion_navigation_wrapping() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.cherry_pick_items_mut() = create_test_cherry_pick_items();
        harness.app.set_version(Some("v1.0.0".to_string()));

        let mut state = CompletionState::new();
        let item_count = harness.app.cherry_pick_items().len();

        // Navigate to end
        for _ in 0..item_count {
            ModeState::process_key(&mut state, KeyCode::Down, harness.merge_app_mut()).await;
        }
        // Should wrap to 0
        assert_eq!(state.list_state.selected(), Some(0));

        // Navigate up from 0
        ModeState::process_key(&mut state, KeyCode::Up, harness.merge_app_mut()).await;
        // Should wrap to last item
        assert_eq!(state.list_state.selected(), Some(item_count - 1));
    }
}
