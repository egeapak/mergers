use super::CleanupModeState;
use crate::{
    models::CleanupStatus,
    ui::apps::CleanupApp,
    ui::state::typed::{ModeState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs},
};

#[derive(Debug, Clone, PartialEq)]
enum ResultTab {
    Success,
    Failed,
}

pub struct CleanupResultsState {
    current_tab: ResultTab,
    success_list_state: ListState,
    failed_list_state: ListState,
}

impl Default for CleanupResultsState {
    fn default() -> Self {
        Self::new()
    }
}

impl CleanupResultsState {
    pub fn new() -> Self {
        let mut success_list_state = ListState::default();
        success_list_state.select(Some(0));

        let mut failed_list_state = ListState::default();
        failed_list_state.select(Some(0));

        Self {
            current_tab: ResultTab::Success,
            success_list_state,
            failed_list_state,
        }
    }

    fn switch_tab(&mut self) {
        self.current_tab = match self.current_tab {
            ResultTab::Success => ResultTab::Failed,
            ResultTab::Failed => ResultTab::Success,
        };
    }

    fn get_success_branches<'a>(
        &self,
        app: &'a CleanupApp,
    ) -> Vec<&'a crate::models::CleanupBranch> {
        app.cleanup_branches()
            .iter()
            .filter(|b| b.selected && matches!(b.status, CleanupStatus::Success))
            .collect()
    }

    fn get_failed_branches<'a>(
        &self,
        app: &'a CleanupApp,
    ) -> Vec<&'a crate::models::CleanupBranch> {
        app.cleanup_branches()
            .iter()
            .filter(|b| b.selected && matches!(b.status, CleanupStatus::Failed(_)))
            .collect()
    }

    fn next(&mut self, app: &CleanupApp) {
        let count = match self.current_tab {
            ResultTab::Success => self.get_success_branches(app).len(),
            ResultTab::Failed => self.get_failed_branches(app).len(),
        };

        if count == 0 {
            return;
        }

        let list_state = match self.current_tab {
            ResultTab::Success => &mut self.success_list_state,
            ResultTab::Failed => &mut self.failed_list_state,
        };

        let i = match list_state.selected() {
            Some(i) => {
                if i >= count - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        list_state.select(Some(i));
    }

    fn previous(&mut self, app: &CleanupApp) {
        let count = match self.current_tab {
            ResultTab::Success => self.get_success_branches(app).len(),
            ResultTab::Failed => self.get_failed_branches(app).len(),
        };

        if count == 0 {
            return;
        }

        let list_state = match self.current_tab {
            ResultTab::Success => &mut self.success_list_state,
            ResultTab::Failed => &mut self.failed_list_state,
        };

        let i = match list_state.selected() {
            Some(i) => {
                if i == 0 {
                    count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        list_state.select(Some(i));
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for CleanupResultsState {
    type Mode = CleanupModeState;

    fn ui(&mut self, f: &mut Frame, app: &CleanupApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("Cleanup Mode - Results")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Tabs
        let success_count = self.get_success_branches(app).len();
        let failed_count = self.get_failed_branches(app).len();

        let tab_titles = vec![
            format!("✅ Deleted ({}) ", success_count),
            format!("❌ Failed ({})", failed_count),
        ];

        let tabs = Tabs::new(tab_titles)
            .block(Block::default().borders(Borders::ALL).title("Results"))
            .select(match self.current_tab {
                ResultTab::Success => 0,
                ResultTab::Failed => 1,
            })
            .style(Style::default().fg(Color::White))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(tabs, chunks[1]);

        // Content based on current tab
        match self.current_tab {
            ResultTab::Success => {
                let branches = self.get_success_branches(app);
                let items: Vec<ListItem> = branches
                    .iter()
                    .map(|branch| {
                        let content = format!(
                            "✅ {} (target: {}, version: {})",
                            branch.name, branch.target, branch.version
                        );
                        ListItem::new(content).style(Style::default().fg(Color::Green))
                    })
                    .collect();

                if items.is_empty() {
                    let empty = Paragraph::new("No branches were successfully deleted.")
                        .style(Style::default().fg(Color::DarkGray))
                        .alignment(Alignment::Center)
                        .block(Block::default().borders(Borders::ALL).title("Deleted"));
                    f.render_widget(empty, chunks[2]);
                } else {
                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title("Deleted"))
                        .highlight_style(
                            Style::default()
                                .bg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("→ ");
                    f.render_stateful_widget(list, chunks[2], &mut self.success_list_state);
                }
            }
            ResultTab::Failed => {
                let branches = self.get_failed_branches(app);
                let items: Vec<ListItem> = branches
                    .iter()
                    .map(|branch| {
                        let error = if let CleanupStatus::Failed(e) = &branch.status {
                            e.as_str()
                        } else {
                            "Unknown error"
                        };
                        let content = format!(
                            "❌ {} (target: {}, version: {}) - {}",
                            branch.name, branch.target, branch.version, error
                        );
                        ListItem::new(content).style(Style::default().fg(Color::Red))
                    })
                    .collect();

                if items.is_empty() {
                    let empty = Paragraph::new(
                        "No failures - all selected branches were successfully deleted!",
                    )
                    .style(Style::default().fg(Color::Green))
                    .alignment(Alignment::Center)
                    .block(Block::default().borders(Borders::ALL).title("Failed"));
                    f.render_widget(empty, chunks[2]);
                } else {
                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title("Failed"))
                        .highlight_style(
                            Style::default()
                                .bg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("→ ");
                    f.render_stateful_widget(list, chunks[2], &mut self.failed_list_state);
                }
            }
        }

        // Help text
        let help_lines = vec![Line::from(vec![
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Switch view  "),
            Span::styled(
                "↑/↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Navigate  "),
            Span::styled(
                "q",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Exit"),
        ])];

        let help = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title("Help"));
        f.render_widget(help, chunks[3]);
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut CleanupApp,
    ) -> StateChange<CleanupModeState> {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Tab => {
                self.switch_tab();
                StateChange::Keep
            }
            KeyCode::Up => {
                self.previous(app);
                StateChange::Keep
            }
            KeyCode::Down => {
                self.next(app);
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "CleanupResults"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::CleanupBranch, models::CleanupStatus, ui::testing::*};
    use insta::assert_snapshot;

    /// # Cleanup Results Success Tab Test
    ///
    /// Tests the results screen showing successfully deleted branches.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds branches with success status
    /// - Renders the results screen on the Success tab
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Results" title
    /// - Should show "✅ Deleted (N)" tab as selected
    /// - Should list all successfully deleted branches
    /// - Should display help text with tab switching and navigation instructions
    #[test]
    fn test_results_success_tab() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add branches with success status
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.3".to_string(),
                    target: "main".to_string(),
                    version: "6.6.3".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: false, // Not selected, should not appear
                    status: CleanupStatus::Success,
                },
            ];

            let state = CleanupResultsState::new();
            harness.render_cleanup_state(&mut CleanupModeState::Results(state));
            assert_snapshot!("success_tab", harness.backend());
        });
    }

    /// # Cleanup Results Failed Tab Test
    ///
    /// Tests the results screen showing failed deletions.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds branches with failed status
    /// - Switches to the Failed tab
    /// - Renders the results screen
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Results" title
    /// - Should show "❌ Failed (N)" tab as selected
    /// - Should list all failed deletions with error messages
    /// - Should display help text
    #[test]
    fn test_results_failed_tab() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add branches with failed status
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Failed("Branch is checked out".to_string()),
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Failed("Protected branch".to_string()),
                },
            ];

            let mut state = CleanupResultsState::new();
            state.current_tab = ResultTab::Failed;

            harness.render_cleanup_state(&mut CleanupModeState::Results(state));
            assert_snapshot!("failed_tab", harness.backend());
        });
    }

    /// # Cleanup Results Mixed Results Test
    ///
    /// Tests the results screen with both successes and failures.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds branches with mixed success and failed statuses
    /// - Renders the results screen on the Success tab
    ///
    /// ## Expected Outcome
    /// - Should display both tabs with correct counts
    /// - Should show only successful deletions on Success tab
    /// - Should allow navigation between tabs
    #[test]
    fn test_results_mixed() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add branches with mixed statuses
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.3".to_string(),
                    target: "main".to_string(),
                    version: "6.6.3".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Failed("Branch is checked out".to_string()),
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
            ];

            let state = CleanupResultsState::new();
            harness.render_cleanup_state(&mut CleanupModeState::Results(state));
            assert_snapshot!("mixed_results", harness.backend());
        });
    }

    /// # Cleanup Results No Failures Test
    ///
    /// Tests the results screen when all deletions succeeded.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds branches with only success status
    /// - Switches to the Failed tab
    /// - Renders the results screen showing empty failures
    ///
    /// ## Expected Outcome
    /// - Should display "❌ Failed (0)" tab
    /// - Should show message "No failures - all selected branches were successfully deleted!"
    #[test]
    fn test_results_no_failures() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add branches with only success status
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.3".to_string(),
                    target: "main".to_string(),
                    version: "6.6.3".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Success,
                },
            ];

            let mut state = CleanupResultsState::new();
            state.current_tab = ResultTab::Failed;

            harness.render_cleanup_state(&mut CleanupModeState::Results(state));
            assert_snapshot!("no_failures", harness.backend());
        });
    }
}
