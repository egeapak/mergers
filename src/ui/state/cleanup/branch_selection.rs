use super::CleanupModeState;
use crate::{
    ui::apps::CleanupApp,
    ui::state::CleanupExecutionState,
    ui::state::typed::{ModeState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

pub struct CleanupBranchSelectionState {
    table_state: TableState,
}

impl Default for CleanupBranchSelectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl CleanupBranchSelectionState {
    pub fn new() -> Self {
        let mut state = Self {
            table_state: TableState::default(),
        };
        state.table_state.select(Some(0));
        state
    }

    fn next(&mut self, app: &CleanupApp) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= app.cleanup_branches().len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn previous(&mut self, app: &CleanupApp) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    app.cleanup_branches().len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn toggle_selection(&mut self, app: &mut CleanupApp) {
        if let Some(i) = self.table_state.selected()
            && i < app.cleanup_branches().len()
        {
            app.cleanup_branches_mut()[i].selected = !app.cleanup_branches()[i].selected;
        }
    }

    fn select_all_merged(&mut self, app: &mut CleanupApp) {
        for branch in app.cleanup_branches_mut() {
            if branch.is_merged {
                branch.selected = true;
            }
        }
    }

    fn deselect_all(&mut self, app: &mut CleanupApp) {
        for branch in app.cleanup_branches_mut() {
            branch.selected = false;
        }
    }

    fn get_selected_count(&self, app: &CleanupApp) -> usize {
        app.cleanup_branches().iter().filter(|b| b.selected).count()
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for CleanupBranchSelectionState {
    type Mode = CleanupModeState;

    fn ui(&mut self, f: &mut Frame, app: &CleanupApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(f.area());

        // Title
        let selected_count = self.get_selected_count(app);
        let title_text = format!(
            "Cleanup Mode - Select Branches to Delete ({} selected)",
            selected_count
        );
        let title = Paragraph::new(title_text)
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Branch table
        let header_cells = ["", "Branch", "Target", "Version", "Status"]
            .iter()
            .map(|h| {
                Cell::from(*h).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let header = Row::new(header_cells).height(1).bottom_margin(1);

        let rows = app.cleanup_branches().iter().map(|branch| {
            let checkbox = if branch.selected { "☑" } else { "☐" };
            let status = if branch.is_merged {
                Span::styled("Merged", Style::default().fg(Color::Green))
            } else {
                Span::styled("Not Merged", Style::default().fg(Color::Yellow))
            };

            let cells = vec![
                Cell::from(checkbox),
                Cell::from(branch.name.as_str()),
                Cell::from(branch.target.as_str()),
                Cell::from(branch.version.as_str()),
                Cell::from(status),
            ];

            Row::new(cells).height(1)
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Min(30),
                Constraint::Length(15),
                Constraint::Length(15),
                Constraint::Length(15),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Branches"))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("→ ");

        f.render_stateful_widget(table, chunks[1], &mut self.table_state);

        // Help text
        let help_lines = vec![
            Line::from(vec![
                Span::styled(
                    "↑/↓",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Navigate  "),
                Span::styled(
                    "Space",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Toggle selection  "),
                Span::styled(
                    "a",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Select all merged"),
            ]),
            Line::from(vec![
                Span::styled(
                    "d",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Deselect all  "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Proceed to cleanup  "),
                Span::styled(
                    "q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Exit"),
            ]),
        ];

        let help = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title("Help"));
        f.render_widget(help, chunks[2]);
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut CleanupApp,
    ) -> StateChange<CleanupModeState> {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Up => {
                self.previous(app);
                StateChange::Keep
            }
            KeyCode::Down => {
                self.next(app);
                StateChange::Keep
            }
            KeyCode::Char(' ') => {
                self.toggle_selection(app);
                StateChange::Keep
            }
            KeyCode::Char('a') => {
                self.select_all_merged(app);
                StateChange::Keep
            }
            KeyCode::Char('d') => {
                self.deselect_all(app);
                StateChange::Keep
            }
            KeyCode::Enter => {
                let selected_count = self.get_selected_count(app);
                if selected_count == 0 {
                    // No branches selected, stay in this state
                    StateChange::Keep
                } else {
                    // Proceed to cleanup execution
                    StateChange::Change(CleanupModeState::Execution(CleanupExecutionState::new()))
                }
            }
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "CleanupBranchSelection"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::CleanupBranch, models::CleanupStatus, ui::testing::*};
    use insta::assert_snapshot;

    /// # Cleanup Branch Selection Empty List Test
    ///
    /// Tests the branch selection screen with no branches.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Renders the branch selection screen with empty branch list
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Select Branches to Delete (0 selected)" title
    /// - Should show empty branch table
    /// - Should display help text with navigation instructions
    #[test]
    fn test_branch_selection_empty() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);
            // Leave cleanup_branches empty
            let state = CleanupBranchSelectionState::new();

            harness.render_cleanup_state(&mut CleanupModeState::BranchSelection(state));
            assert_snapshot!("empty", harness.backend());
        });
    }

    /// # Cleanup Branch Selection With Branches Test
    ///
    /// Tests the branch selection screen with multiple branches.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds several patch branches (merged and not merged)
    /// - Renders the branch selection screen
    ///
    /// ## Expected Outcome
    /// - Should display branch table with checkboxes
    /// - Should show branch metadata (target, version, status)
    /// - Should highlight current selection
    /// - Should display help text with all available actions
    #[test]
    fn test_branch_selection_with_branches() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add sample branches
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: false,
                    status: CleanupStatus::Pending,
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: false,
                    status: CleanupStatus::Pending,
                },
                CleanupBranch {
                    name: "patch/main-6.6.0".to_string(),
                    target: "main".to_string(),
                    version: "6.6.0".to_string(),
                    is_merged: false,
                    selected: false,
                    status: CleanupStatus::Pending,
                },
            ];

            let state = CleanupBranchSelectionState::new();
            harness.render_cleanup_state(&mut CleanupModeState::BranchSelection(state));
            assert_snapshot!("with_branches", harness.backend());
        });
    }

    /// # Cleanup Branch Selection With Selections Test
    ///
    /// Tests the branch selection screen with some branches selected.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Adds several patch branches with some pre-selected
    /// - Renders the branch selection screen
    ///
    /// ## Expected Outcome
    /// - Should display checked checkboxes for selected branches
    /// - Should show "(N selected)" in the title
    /// - Should display all branch information correctly
    #[test]
    fn test_branch_selection_with_selections() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config);

            // Add sample branches with some selected
            *harness.app.cleanup_branches_mut() = vec![
                CleanupBranch {
                    name: "patch/main-6.6.2".to_string(),
                    target: "main".to_string(),
                    version: "6.6.2".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Pending,
                },
                CleanupBranch {
                    name: "patch/next-6.6.1".to_string(),
                    target: "next".to_string(),
                    version: "6.6.1".to_string(),
                    is_merged: true,
                    selected: true,
                    status: CleanupStatus::Pending,
                },
                CleanupBranch {
                    name: "patch/main-6.6.0".to_string(),
                    target: "main".to_string(),
                    version: "6.6.0".to_string(),
                    is_merged: false,
                    selected: false,
                    status: CleanupStatus::Pending,
                },
            ];

            let state = CleanupBranchSelectionState::new();
            harness.render_cleanup_state(&mut CleanupModeState::BranchSelection(state));
            assert_snapshot!("with_selections", harness.backend());
        });
    }
}
