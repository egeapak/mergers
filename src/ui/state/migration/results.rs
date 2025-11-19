use crate::ui::App;
use crate::ui::state::{AppState, StateChange};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationTab {
    Eligible,
    Unsure,
    NotMerged,
}

pub struct MigrationState {
    pub current_tab: MigrationTab,
    pub eligible_list_state: ListState,
    pub unsure_list_state: ListState,
    pub not_merged_list_state: ListState,
    pub show_details: bool,
}

impl Default for MigrationState {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationState {
    pub fn new() -> Self {
        let mut eligible_list_state = ListState::default();
        eligible_list_state.select(Some(0));

        Self {
            current_tab: MigrationTab::Eligible,
            eligible_list_state,
            unsure_list_state: ListState::default(),
            not_merged_list_state: ListState::default(),
            show_details: false,
        }
    }

    fn get_current_list_state(&mut self) -> &mut ListState {
        match self.current_tab {
            MigrationTab::Eligible => &mut self.eligible_list_state,
            MigrationTab::Unsure => &mut self.unsure_list_state,
            MigrationTab::NotMerged => &mut self.not_merged_list_state,
        }
    }

    fn get_current_prs_count(&self, app: &App) -> usize {
        if let Some(analysis) = &app.migration_analysis {
            match self.current_tab {
                MigrationTab::Eligible => analysis.eligible_prs.len(),
                MigrationTab::Unsure => analysis.unsure_prs.len(),
                MigrationTab::NotMerged => analysis.not_merged_prs.len(),
            }
        } else {
            0
        }
    }

    fn move_selection(&mut self, app: &App, direction: i32) {
        let count = self.get_current_prs_count(app);

        if count == 0 {
            return;
        }

        let current_list = self.get_current_list_state();
        let current = current_list.selected().unwrap_or(0);
        let new_index = if direction > 0 {
            (current + 1) % count
        } else if current == 0 {
            count - 1
        } else {
            current - 1
        };
        current_list.select(Some(new_index));
    }

    fn switch_tab(&mut self, app: &App, direction: i32) {
        self.current_tab = match self.current_tab {
            MigrationTab::Eligible => {
                if direction > 0 {
                    MigrationTab::Unsure
                } else {
                    MigrationTab::NotMerged
                }
            }
            MigrationTab::Unsure => {
                if direction > 0 {
                    MigrationTab::NotMerged
                } else {
                    MigrationTab::Eligible
                }
            }
            MigrationTab::NotMerged => {
                if direction > 0 {
                    MigrationTab::Eligible
                } else {
                    MigrationTab::Unsure
                }
            }
        };

        // Ensure the new tab has a valid selection
        let count = self.get_current_prs_count(app);
        if count > 0 {
            let current_list = self.get_current_list_state();
            if current_list.selected().is_none() {
                current_list.select(Some(0));
            }
        }
    }

    fn get_current_pr<'a>(
        &self,
        app: &'a App,
    ) -> Option<&'a crate::models::PullRequestWithWorkItems> {
        if let Some(analysis) = &app.migration_analysis {
            let list_state = match self.current_tab {
                MigrationTab::Eligible => &self.eligible_list_state,
                MigrationTab::Unsure => &self.unsure_list_state,
                MigrationTab::NotMerged => &self.not_merged_list_state,
            };

            if let Some(selected) = list_state.selected() {
                match self.current_tab {
                    MigrationTab::Eligible => analysis.eligible_prs.get(selected),
                    MigrationTab::Unsure => analysis.unsure_prs.get(selected),
                    MigrationTab::NotMerged => analysis.not_merged_prs.get(selected),
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    fn open_current_pr(&self, app: &App) {
        if let Some(pr) = self.get_current_pr(app) {
            app.open_pr_in_browser(pr.pr.id);
        }
    }

    fn toggle_pr_eligibility(&self, app: &mut App, pr_id: i32) {
        // Get current manual override state
        let current_override = app.has_manual_override(pr_id);

        match self.current_tab {
            MigrationTab::Eligible => {
                // In eligible tab: eligible → not eligible → no override (back to eligible)
                match current_override {
                    None => {
                        // No override (naturally eligible) → mark as not eligible
                        app.mark_pr_as_not_eligible(pr_id);
                    }
                    Some(false) => {
                        // Manually marked not eligible → remove override (back to natural state)
                        app.remove_manual_override(pr_id);
                    }
                    Some(true) => {
                        // This shouldn't happen in eligible tab, but handle gracefully
                        // Manually marked eligible → mark as not eligible
                        app.mark_pr_as_not_eligible(pr_id);
                    }
                }
            }
            MigrationTab::NotMerged => {
                // In not merged tab: not eligible → eligible → no override (back to not eligible)
                match current_override {
                    None => {
                        // No override (naturally not eligible) → mark as eligible
                        app.mark_pr_as_eligible(pr_id);
                    }
                    Some(true) => {
                        // Manually marked eligible → remove override (back to natural state)
                        app.remove_manual_override(pr_id);
                    }
                    Some(false) => {
                        // This shouldn't happen in not merged tab, but handle gracefully
                        // Manually marked not eligible → mark as eligible
                        app.mark_pr_as_eligible(pr_id);
                    }
                }
            }
            MigrationTab::Unsure => {
                // In unsure tab: work like not merged tab
                // unsure → eligible → no override (back to unsure)
                match current_override {
                    None => {
                        // No override (naturally unsure) → mark as eligible
                        app.mark_pr_as_eligible(pr_id);
                    }
                    Some(true) => {
                        // Manually marked eligible → remove override (back to natural state)
                        app.remove_manual_override(pr_id);
                    }
                    Some(false) => {
                        // This shouldn't happen in unsure tab, but handle gracefully
                        // Manually marked not eligible → mark as eligible
                        app.mark_pr_as_eligible(pr_id);
                    }
                }
            }
        }
    }

    fn render_tabs(&self, f: &mut Frame, app: &App, area: Rect) {
        let analysis = app.migration_analysis.as_ref().unwrap();

        let tab_titles = vec![
            format!("✅ Eligible ({})", analysis.eligible_prs.len()),
            format!("❓ Unsure ({})", analysis.unsure_prs.len()),
            format!("❌ Not Merged ({})", analysis.not_merged_prs.len()),
        ];

        let tabs = Tabs::new(tab_titles)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(Color::Yellow).bold())
            .select(match self.current_tab {
                MigrationTab::Eligible => 0,
                MigrationTab::Unsure => 1,
                MigrationTab::NotMerged => 2,
            });

        f.render_widget(tabs, area);
    }

    fn render_pr_list(&mut self, f: &mut Frame, app: &App, area: Rect) {
        let analysis = app.migration_analysis.as_ref().unwrap();

        let (prs, title, color) = match self.current_tab {
            MigrationTab::Eligible => (
                &analysis.eligible_prs,
                "Eligible PRs - Ready for tagging",
                Color::Green,
            ),
            MigrationTab::Unsure => (
                &analysis.unsure_prs,
                "Unsure PRs - Require manual review",
                Color::Yellow,
            ),
            MigrationTab::NotMerged => (
                &analysis.not_merged_prs,
                "Not Merged PRs - Not ready for migration",
                Color::Red,
            ),
        };

        let items: Vec<ListItem> = prs
            .iter()
            .map(|pr| {
                // Check if this PR has a manual override and show what Space will do
                let (override_indicator, space_action) = match app.has_manual_override(pr.pr.id) {
                    Some(true) => {
                        let action = match self.current_tab {
                            MigrationTab::Eligible => " → Not Eligible", // will mark not eligible
                            MigrationTab::Unsure => " → Reset",          // will reset override
                            MigrationTab::NotMerged => " → Reset",       // will reset override
                        };
                        (" ✅ [Manual]", action)
                    }
                    Some(false) => {
                        let action = match self.current_tab {
                            MigrationTab::Eligible => " → Reset",     // will reset override
                            MigrationTab::Unsure => " → Eligible",    // will mark eligible
                            MigrationTab::NotMerged => " → Eligible", // will mark eligible
                        };
                        (" ❌ [Manual Override]", action)
                    }
                    None => {
                        let action = match self.current_tab {
                            MigrationTab::Eligible => " → Not Eligible", // will mark not eligible
                            MigrationTab::Unsure => " → Eligible",       // will mark eligible
                            MigrationTab::NotMerged => " → Eligible",    // will mark eligible
                        };
                        ("", action)
                    }
                };

                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!("#{}", pr.pr.id),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::raw(&pr.pr.title),
                        Span::styled(
                            override_indicator,
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(space_action, Style::default().fg(Color::Cyan)),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("  By: {}", pr.pr.created_by.display_name),
                            Style::default().fg(Color::Gray),
                        ),
                        Span::raw(" | "),
                        Span::styled(
                            format!("Work Items: {}", pr.work_items.len()),
                            Style::default().fg(Color::Gray),
                        ),
                    ]),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(color)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let current_list = self.get_current_list_state();
        f.render_stateful_widget(list, area, current_list);
    }

    fn render_details(&self, f: &mut Frame, app: &App, area: Rect) {
        if let Some(pr) = self.get_current_pr(app) {
            let analysis = app.migration_analysis.as_ref().unwrap();

            let mut details = vec![
                Line::from(vec![Span::styled(
                    "PR Details:",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![Span::raw(format!("ID: #{}", pr.pr.id))]),
                Line::from(vec![Span::raw(format!("Title: {}", pr.pr.title))]),
                Line::from(vec![Span::raw(format!(
                    "Created By: {}",
                    pr.pr.created_by.display_name
                ))]),
                Line::from(""),
            ];

            // Add work items information
            if pr.work_items.is_empty() {
                details.push(Line::from(vec![Span::styled(
                    "Work Items: None",
                    Style::default().fg(Color::Gray),
                )]));
            } else {
                details.push(Line::from(vec![Span::styled(
                    "Work Items:",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]));
                for work_item in &pr.work_items {
                    let state = work_item.fields.state.as_deref().unwrap_or("Unknown");
                    let color = if analysis.terminal_states.contains(&state.to_string()) {
                        Color::Green
                    } else {
                        Color::Red
                    };
                    details.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            format!("#{}", work_item.id),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(" - "),
                        Span::raw(work_item.fields.title.as_deref().unwrap_or("No title")),
                        Span::raw(" ("),
                        Span::styled(state, Style::default().fg(color)),
                        Span::raw(")"),
                    ]));
                }
            }

            // Add general reason for all PRs using all_details
            if let Some(detail) = analysis.all_details.iter().find(|d| d.pr.pr.id == pr.pr.id)
                && let Some(reason) = &detail.reason
            {
                details.push(Line::from(""));
                details.push(Line::from(vec![Span::styled(
                    "Reason:",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )]));
                details.push(Line::from(vec![Span::raw(reason)]));
            }

            // Add unsure reason for unsure PRs (legacy support)
            if self.current_tab == MigrationTab::Unsure
                && let Some(unsure_detail) = analysis
                    .unsure_details
                    .iter()
                    .find(|d| d.pr.pr.id == pr.pr.id)
                && let Some(reason) = &unsure_detail.unsure_reason
            {
                details.push(Line::from(""));
                details.push(Line::from(vec![Span::styled(
                    "Unsure Reason:",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )]));
                details.push(Line::from(vec![Span::raw(reason)]));
            }

            let paragraph = Paragraph::new(details)
                .block(Block::default().borders(Borders::ALL).title("Details"))
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, area);
        }
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help_text = vec![
            Line::from(vec![Span::styled(
                "Navigation:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from("  ↑/↓ - Navigate PRs | ←/→ - Switch tabs | o - Open PR in browser"),
            Line::from("  d - Toggle details | q - Quit"),
            Line::from(vec![Span::styled(
                "Toggle Eligibility:",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from("  Space - Toggle PR eligibility (cycles through states)"),
            Line::from(vec![Span::styled(
                "Next Step:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from("  Enter - Proceed to Version Input for Tagging"),
        ];

        let paragraph = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}

#[async_trait]
impl AppState for MigrationState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        if app.migration_analysis.is_none() {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tabs
                Constraint::Min(10),   // Main content
                Constraint::Length(9), // Help
            ])
            .split(f.area());

        // Render tabs
        self.render_tabs(f, app, chunks[0]);

        // Split main content area
        let main_chunks = if self.show_details {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[1])
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(chunks[1])
        };

        // Render PR list
        self.render_pr_list(f, app, main_chunks[0]);

        // Render details if enabled
        if self.show_details && main_chunks.len() > 1 {
            self.render_details(f, app, main_chunks[1]);
        }

        // Render help
        self.render_help(f, chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Up => {
                self.move_selection(app, -1);
                StateChange::Keep
            }
            KeyCode::Down => {
                self.move_selection(app, 1);
                StateChange::Keep
            }
            KeyCode::Left => {
                self.switch_tab(app, -1);
                StateChange::Keep
            }
            KeyCode::Right => {
                self.switch_tab(app, 1);
                StateChange::Keep
            }
            KeyCode::Char('o') => {
                // Open PR in browser
                self.open_current_pr(app);
                StateChange::Keep
            }
            KeyCode::Char('d') => {
                self.show_details = !self.show_details;
                StateChange::Keep
            }
            KeyCode::Char(' ') => {
                // Toggle eligibility based on current tab and override state
                if let Some(pr) = self.get_current_pr(app) {
                    self.toggle_pr_eligibility(app, pr.pr.id);
                }
                StateChange::Keep
            }
            KeyCode::Enter => {
                // Proceed to version input for tagging
                StateChange::Change(Box::new(super::MigrationVersionInputState::new()))
            }
            _ => StateChange::Keep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{TuiTestHarness, create_test_config_migration, create_test_migration_analysis},
    };
    use insta::assert_snapshot;

    /// # Migration Results State - Display
    ///
    /// Tests the migration results screen with analysis data.
    ///
    /// ## Test Scenario
    /// - Creates a migration results state
    /// - Loads migration analysis data
    /// - Renders the results display
    ///
    /// ## Expected Outcome
    /// - Should display eligible PRs
    /// - Should display not eligible PRs
    /// - Should show statistics
    /// - Should display help text
    #[test]
    fn test_migration_results_display() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            harness.app.migration_analysis = Some(create_test_migration_analysis());

            let state = Box::new(MigrationState::new());
            harness.render_state(state);

            assert_snapshot!("display", harness.backend());
        });
    }

    /// # Migration Results State - Bottom Bar Visibility
    ///
    /// Tests that the bottom help bar is fully visible with all content.
    ///
    /// ## Test Scenario
    /// - Creates a migration results state
    /// - Loads migration analysis data
    /// - Renders the results display
    /// - Verifies the bottom bar contains all expected help text
    ///
    /// ## Expected Outcome
    /// - Should display complete navigation section
    /// - Should display complete toggle eligibility section
    /// - Should display complete next step section with Enter key instruction
    /// - All text should be within the visible terminal area
    #[test]
    fn test_migration_results_bottom_bar_fully_visible() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            harness.app.migration_analysis = Some(create_test_migration_analysis());

            let state = Box::new(MigrationState::new());
            harness.render_state(state);

            // Get the rendered output
            let output = harness.backend().to_string();

            // Verify all help sections are present
            assert!(
                output.contains("Navigation:"),
                "Help bar should contain Navigation section"
            );
            assert!(
                output.contains("Toggle Eligibility:"),
                "Help bar should contain Toggle Eligibility section"
            );
            assert!(
                output.contains("Next Step:"),
                "Help bar should contain Next Step section"
            );
            assert!(
                output.contains("Enter - Proceed to Version Input for Tagging"),
                "Help bar should contain Enter key instruction for proceeding"
            );

            // Snapshot the full display to verify visual layout
            assert_snapshot!("bottom_bar_fully_visible", harness.backend());
        });
    }

    /// # Migration Results State - With Manual Eligible Override
    ///
    /// Tests the migration results screen showing manual eligible overrides.
    ///
    /// ## Test Scenario
    /// - Creates a migration results state
    /// - Loads migration analysis with PRs
    /// - Manually marks a not-merged PR as eligible using manual override
    /// - Renders the results showing the ✅ [Manual] indicator
    ///
    /// ## Expected Outcome
    /// - Should display the not-merged PR in eligible tab with ✅ [Manual] indicator
    /// - Should show the action indicator "→ Not Eligible" for what Space will do
    #[test]
    fn test_migration_results_manual_eligible_override() {
        use std::collections::HashSet;
        use crate::models::ManualOverrides;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            let mut analysis = create_test_migration_analysis();

            // Create manual overrides with PR 102 marked as eligible
            let mut marked_as_eligible = HashSet::new();
            marked_as_eligible.insert(102);
            analysis.manual_overrides = ManualOverrides {
                marked_as_eligible,
                marked_as_not_eligible: HashSet::new(),
            };

            // Move PR 102 from not_merged to eligible to simulate the manual override
            if let Some(pr) = analysis.not_merged_prs.first().cloned() {
                analysis.eligible_prs.push(pr);
                analysis.not_merged_prs.clear();
            }

            harness.app.migration_analysis = Some(analysis);

            let state = Box::new(MigrationState::new());
            harness.render_state(state);

            assert_snapshot!("manual_eligible_override", harness.backend());
        });
    }

    /// # Migration Results State - With Manual Not-Eligible Override
    ///
    /// Tests the migration results screen showing manual not-eligible overrides.
    ///
    /// ## Test Scenario
    /// - Creates a migration results state
    /// - Loads migration analysis with PRs
    /// - Manually marks an eligible PR as not-eligible using manual override
    /// - Switches to not-merged tab
    /// - Renders the results showing the ❌ [Manual Override] indicator
    ///
    /// ## Expected Outcome
    /// - Should display the PR in not-merged tab with ❌ [Manual Override] indicator
    /// - Should show the action indicator "→ Eligible" for what Space will do
    #[test]
    fn test_migration_results_manual_not_eligible_override() {
        use std::collections::HashSet;
        use crate::models::ManualOverrides;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config);

            let mut analysis = create_test_migration_analysis();

            // Create manual overrides with PR 100 marked as not eligible
            let mut marked_as_not_eligible = HashSet::new();
            marked_as_not_eligible.insert(100);
            analysis.manual_overrides = ManualOverrides {
                marked_as_eligible: HashSet::new(),
                marked_as_not_eligible,
            };

            // Move PR 100 from eligible to not_merged to simulate the manual override
            if let Some(pr) = analysis.eligible_prs.first().cloned() {
                analysis.not_merged_prs.push(pr);
                analysis.eligible_prs.remove(0);
            }

            harness.app.migration_analysis = Some(analysis);

            let mut state = MigrationState::new();
            // Switch to not-merged tab to see the manual override
            state.current_tab = MigrationTab::NotMerged;
            state.not_merged_list_state.select(Some(0));

            harness.render_state(Box::new(state));

            assert_snapshot!("manual_not_eligible_override", harness.backend());
        });
    }
}
