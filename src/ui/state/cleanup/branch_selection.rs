use crate::{
    ui::App,
    ui::state::{AppState, CleanupExecutionState, StateChange},
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

    fn next(&mut self, app: &App) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= app.cleanup_branches.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn previous(&mut self, app: &App) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    app.cleanup_branches.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn toggle_selection(&mut self, app: &mut App) {
        if let Some(i) = self.table_state.selected()
            && i < app.cleanup_branches.len()
        {
            app.cleanup_branches[i].selected = !app.cleanup_branches[i].selected;
        }
    }

    fn select_all_merged(&mut self, app: &mut App) {
        for branch in &mut app.cleanup_branches {
            if branch.is_merged {
                branch.selected = true;
            }
        }
    }

    fn deselect_all(&mut self, app: &mut App) {
        for branch in &mut app.cleanup_branches {
            branch.selected = false;
        }
    }

    fn get_selected_count(&self, app: &App) -> usize {
        app.cleanup_branches.iter().filter(|b| b.selected).count()
    }
}

#[async_trait]
impl AppState for CleanupBranchSelectionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
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

        let rows = app.cleanup_branches.iter().map(|branch| {
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
                Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
                Span::raw(": Navigate  "),
                Span::styled("Space", Style::default().fg(Color::Yellow)),
                Span::raw(": Toggle selection  "),
                Span::styled("a", Style::default().fg(Color::Yellow)),
                Span::raw(": Select all merged"),
            ]),
            Line::from(vec![
                Span::styled("d", Style::default().fg(Color::Yellow)),
                Span::raw(": Deselect all  "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(": Proceed to cleanup  "),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(": Exit"),
            ]),
        ];

        let help = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title("Help"));
        f.render_widget(help, chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
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
                    StateChange::Change(Box::new(CleanupExecutionState::new()))
                }
            }
            _ => StateChange::Keep,
        }
    }
}
