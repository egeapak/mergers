use crate::{
    ui::App,
    ui::state::{AppState, StateChange, VersionInputState},
};
use async_trait::async_trait;
use chrono::DateTime;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, TableState},
};

pub struct PullRequestSelectionState {
    table_state: TableState,
}

impl PullRequestSelectionState {
    pub fn new() -> Self {
        Self {
            table_state: TableState::default(),
        }
    }

    fn initialize_selection(&mut self, app: &App) {
        if !app.pull_requests.is_empty() && self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        }
    }

    fn next(&mut self, app: &App) {
        if app.pull_requests.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= app.pull_requests.len() - 1 {
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
        if app.pull_requests.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    app.pull_requests.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn toggle_selection(&mut self, app: &mut App) {
        if let Some(i) = self.table_state.selected() {
            if let Some(pr) = app.pull_requests.get_mut(i) {
                pr.selected = !pr.selected;
            }
        }
    }


}

#[async_trait]
impl AppState for PullRequestSelectionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        // Initialize selection if not already set
        self.initialize_selection(app);

        // Handle empty PR list
        if app.pull_requests.is_empty() {
            let empty_message = Paragraph::new("No pull requests found without merged tags.\n\nPress 'q' to quit.")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("No Pull Requests"))
                .alignment(Alignment::Center);
            f.render_widget(empty_message, f.area());
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(f.area());
        // Create table headers
        let header_cells = ["", "PR #", "Date", "Title", "Author", "Work Items"]
            .iter()
            .map(|h| {
                Cell::from(*h).style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let header = Row::new(header_cells).height(1);

        // Create table rows
        let rows: Vec<Row> = app
            .pull_requests
            .iter()
            .map(|pr_with_wi| {
                let selected = if pr_with_wi.selected { "✓" } else { " " };

                let date = if let Some(closed_date) = &pr_with_wi.pr.closed_date {
                    if let Ok(date) = DateTime::parse_from_rfc3339(closed_date) {
                        date.format("%Y-%m-%d").to_string()
                    } else {
                        "Active".to_string()
                    }
                } else {
                    "Active".to_string()
                };

                let work_items = if !pr_with_wi.work_items.is_empty() {
                    pr_with_wi
                        .work_items
                        .iter()
                        .map(|wi| {
                            let state = wi.fields.state.as_deref().unwrap_or("Unknown");
                            format!("#{} ({})", wi.id, state)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    String::new()
                };

                let cells = vec![
                    Cell::from(selected).style(Style::default().fg(Color::Green)),
                    Cell::from(format!("{}", pr_with_wi.pr.id))
                        .style(Style::default().fg(Color::Cyan)),
                    Cell::from(date),
                    Cell::from(pr_with_wi.pr.title.clone()),
                    Cell::from(pr_with_wi.pr.created_by.display_name.clone())
                        .style(Style::default().fg(Color::Yellow)),
                    Cell::from(work_items)
                        .style(Style::default().fg(get_work_items_color(&pr_with_wi.work_items))),
                ];

                Row::new(cells).height(1)
            })
            .collect();

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(12),
                Constraint::Percentage(30),
                Constraint::Percentage(20),
                Constraint::Percentage(25),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Pull Requests"),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol("→ ");

        f.render_stateful_widget(table, chunks[0], &mut self.table_state);

        let help = List::new(vec![
            ListItem::new("↑/↓: Navigate | Space: Toggle selection | Enter: Confirm | p: Open PR | w: Open Work Items | q: Quit"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Help"));

        f.render_widget(help, chunks[1]);
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
            KeyCode::Char('p') => {
                if let Some(i) = self.table_state.selected() {
                    if let Some(pr) = app.pull_requests.get(i) {
                        app.open_pr_in_browser(pr.pr.id);
                    }
                }
                StateChange::Keep
            }
            KeyCode::Char('w') => {
                if let Some(i) = self.table_state.selected() {
                    if let Some(pr) = app.pull_requests.get(i) {
                        app.open_work_items_in_browser(&pr.work_items);
                    }
                }
                StateChange::Keep
            }
            KeyCode::Enter => {
                if app.get_selected_prs().is_empty() {
                    StateChange::Keep
                } else {
                    StateChange::Change(Box::new(VersionInputState::new()))
                }
            }
            _ => StateChange::Keep,
        }
    }
}

fn get_work_items_color(work_items: &[crate::models::WorkItem]) -> Color {
    if work_items.is_empty() {
        return Color::Gray;
    }

    // Return color based on the most important state
    for wi in work_items {
        if let Some(state) = &wi.fields.state {
            match state.as_str() {
                "Next Merged" | "Next Closed" => return get_state_color(state),
                _ => {}
            }
        }
    }

    work_items
        .iter()
        .filter_map(|wi| wi.fields.state.as_deref())
        .next()
        .map(get_state_color)
        .unwrap_or(Color::White)
}

fn get_state_color(state: &str) -> Color {
    match state {
        "Dev Closed" => Color::LightGreen,
        "Closed" => Color::Green,
        "Resolved" => Color::Rgb(255, 165, 0),
        "In Review" => Color::Yellow,
        "New" => Color::Gray,
        "Active" => Color::Blue,
        "Next Merged" => Color::Red,
        "Next Closed" => Color::Magenta,
        "Hold" => Color::Cyan,
        _ => Color::White,
    }
}
