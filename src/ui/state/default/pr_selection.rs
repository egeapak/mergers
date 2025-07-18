use super::{DataLoadingState, VersionInputState};
use crate::{
    models::WorkItemHistory,
    ui::App,
    ui::state::{AppState, StateChange},
    utils::html_to_lines,
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
    work_item_index: usize,
}

impl PullRequestSelectionState {
    pub fn new() -> Self {
        Self {
            table_state: TableState::default(),
            work_item_index: 0,
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
        self.work_item_index = 0; // Reset work item selection when PR changes
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
        self.work_item_index = 0; // Reset work item selection when PR changes
    }

    fn toggle_selection(&mut self, app: &mut App) {
        if let Some(i) = self.table_state.selected() {
            if let Some(pr) = app.pull_requests.get_mut(i) {
                pr.selected = !pr.selected;
            }
        }
    }

    fn next_work_item(&mut self, app: &App) {
        if let Some(pr_index) = self.table_state.selected() {
            if let Some(pr) = app.pull_requests.get(pr_index) {
                if !pr.work_items.is_empty() {
                    self.work_item_index = (self.work_item_index + 1) % pr.work_items.len();
                }
            }
        }
    }

    fn previous_work_item(&mut self, app: &App) {
        if let Some(pr_index) = self.table_state.selected() {
            if let Some(pr) = app.pull_requests.get(pr_index) {
                if !pr.work_items.is_empty() {
                    if self.work_item_index == 0 {
                        self.work_item_index = pr.work_items.len() - 1;
                    } else {
                        self.work_item_index -= 1;
                    }
                }
            }
        }
    }

    fn render_work_item_details(&self, f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
        if let Some(pr_index) = self.table_state.selected() {
            if let Some(pr) = app.pull_requests.get(pr_index) {
                if pr.work_items.is_empty() {
                    let no_items =
                        Paragraph::new("No work items associated with this pull request.")
                            .style(Style::default().fg(Color::Gray))
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("Work Item Details"),
                            )
                            .alignment(Alignment::Center);
                    f.render_widget(no_items, area);
                    return;
                }

                // Ensure work_item_index is within bounds
                let work_item_index = if self.work_item_index < pr.work_items.len() {
                    self.work_item_index
                } else {
                    0
                };

                if let Some(work_item) = pr.work_items.get(work_item_index) {
                    // Create layout for header, history, and content
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(4), // Header (2 lines + borders)
                            Constraint::Length(3), // History (1 line + borders)
                            Constraint::Min(0),    // Description content
                        ])
                        .split(area);

                    // Render header
                    let state = work_item.fields.state.as_deref().unwrap_or("Unknown");
                    let work_item_type = work_item
                        .fields
                        .work_item_type
                        .as_deref()
                        .unwrap_or("Unknown");
                    let assigned_to = work_item
                        .fields
                        .assigned_to
                        .as_ref()
                        .map(|user| user.display_name.as_str())
                        .unwrap_or("Unassigned");
                    let iteration_path = work_item
                        .fields
                        .iteration_path
                        .as_deref()
                        .unwrap_or("Unknown");
                    let title = work_item.fields.title.as_deref().unwrap_or("No title");

                    // Get colors for type and state
                    let type_color = match work_item_type.to_lowercase().as_str() {
                        "task" => Color::Yellow,
                        "bug" => Color::Red,
                        "user story" => Color::Blue,
                        "feature" => Color::Green,
                        _ => Color::White,
                    };

                    let state_color = get_state_color(state);

                    // Create header content with spans for different colors and proper alignment
                    use ratatui::text::{Line, Span};
                    let header_lines = vec![
                        Line::from(vec![
                            Span::styled(
                                format!("{:<11}", work_item_type), // Fixed width for type
                                Style::default().fg(type_color).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!(" #{:<6} ", work_item.id), // Fixed width for ID
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                title,
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]),
                        Line::from(vec![
                            Span::styled(
                                "●",
                                Style::default()
                                    .fg(state_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!(" {:<15}", state), // Fixed width for state
                                Style::default().fg(state_color),
                            ),
                            Span::styled(
                                format!(" | Iteration: {}", iteration_path),
                                Style::default().fg(Color::Gray),
                            ),
                            Span::styled(
                                format!(" | Assigned: {}", assigned_to),
                                Style::default().fg(Color::Yellow),
                            ),
                        ]),
                    ];

                    let header_widget = Paragraph::new(header_lines).block(
                        Block::default().borders(Borders::ALL).title(format!(
                            "Work Item ({}/{})",
                            work_item_index + 1,
                            pr.work_items.len()
                        )),
                    );

                    f.render_widget(header_widget, chunks[0]);

                    // Render history section
                    self.render_work_item_history_linear(f, chunks[1], work_item);

                    // Render description - use repro steps for bugs, description for others
                    let (description_content, description_title) = match work_item_type
                        .to_lowercase()
                        .as_str()
                    {
                        "bug" => {
                            let content = if let Some(repro_steps) = &work_item.fields.repro_steps {
                                if !repro_steps.is_empty() {
                                    repro_steps.clone()
                                } else if let Some(description) = &work_item.fields.description {
                                    if !description.is_empty() {
                                        description.clone()
                                    } else {
                                        "No reproduction steps available.".to_string()
                                    }
                                } else {
                                    "No reproduction steps available.".to_string()
                                }
                            } else if let Some(description) = &work_item.fields.description {
                                if !description.is_empty() {
                                    description.clone()
                                } else {
                                    "No reproduction steps available.".to_string()
                                }
                            } else {
                                "No reproduction steps available.".to_string()
                            };
                            (
                                content,
                                "Reproduction Steps (use ←/→ to navigate work items)",
                            )
                        }
                        _ => {
                            let content = if let Some(description) = &work_item.fields.description {
                                if !description.is_empty() {
                                    description.clone()
                                } else {
                                    "No description available.".to_string()
                                }
                            } else {
                                "No description available.".to_string()
                            };
                            (content, "Description (use ←/→ to navigate work items)")
                        }
                    };

                    // Convert HTML content to ratatui spans
                    let description_lines = html_to_lines(&description_content);

                    let description_widget = Paragraph::new(description_lines)
                        .style(Style::default().fg(Color::White))
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(description_title),
                        )
                        .wrap(ratatui::widgets::Wrap { trim: true });

                    f.render_widget(description_widget, chunks[2]);
                }
            }
        } else {
            let no_selection = Paragraph::new("No pull request selected.")
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Work Item Details"),
                )
                .alignment(Alignment::Center);
            f.render_widget(no_selection, area);
        }
    }

    fn render_work_item_history_linear(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        work_item: &crate::models::WorkItem,
    ) {
        use ratatui::text::{Line, Span};

        let mut history_spans = vec![];

        if work_item.history.is_empty() {
            history_spans.push(Span::styled(
                "No history available",
                Style::default().fg(Color::Gray),
            ));
        } else {
            // Sort history by date (most recent first) and filter to only state changes
            let mut state_changes: Vec<_> = work_item
                .history
                .iter()
                .filter(|h| {
                    h.fields
                        .as_ref()
                        .and_then(|f| f.state.as_ref())
                        .and_then(|s| s.new_value.as_ref())
                        .is_some()
                })
                .cloned()
                .collect();

            // Sort by date from earliest to latest (left to right chronologically)
            // Use System.ChangedDate as primary date source, fall back to revisedDate
            state_changes.sort_by(|a, b| {
                let get_date_string = |entry: &WorkItemHistory| -> Option<String> {
                    // First try System.ChangedDate
                    if let Some(fields) = &entry.fields {
                        if let Some(changed_date) = &fields.changed_date {
                            if let Some(new_date) = &changed_date.new_value {
                                if !new_date.starts_with("9999-01-01") {
                                    return Some(new_date.clone());
                                }
                            }
                        }
                    }
                    // Fall back to revisedDate if not a placeholder
                    if !entry.revised_date.starts_with("9999-01-01") {
                        Some(entry.revised_date.clone())
                    } else {
                        None // No valid date found
                    }
                };

                let a_date = get_date_string(a);
                let b_date = get_date_string(b);

                match (a_date, b_date) {
                    (Some(a_d), Some(b_d)) => a_d.cmp(&b_d), // Normal chronological order
                    (None, Some(_)) => std::cmp::Ordering::Less, // a has unknown date, goes first
                    (Some(_), None) => std::cmp::Ordering::Greater, // b has unknown date, goes first
                    (None, None) => a.rev.cmp(&b.rev), // Both unknown, use revision order
                }
            });

            if state_changes.is_empty() {
                history_spans.push(Span::styled(
                    "No state changes in history",
                    Style::default().fg(Color::Gray),
                ));
            } else {
                // Show first 5 and last 1 entries (Azure DevOps style) in chronological order
                let total_count = state_changes.len();
                let entries_to_show = if total_count <= 6 {
                    state_changes
                } else {
                    let mut entries = Vec::new();
                    entries.extend(state_changes[..5].iter().cloned()); // First 5 (earliest)
                    entries.push(state_changes[total_count - 1].clone()); // Last 1 (latest)
                    entries
                };

                for (i, history_entry) in entries_to_show.iter().enumerate() {
                    // Add separator for omitted entries (after showing first 5, before showing last 1)
                    if i == 5 && total_count > 6 {
                        if !history_spans.is_empty() {
                            history_spans
                                .push(Span::styled(" → ", Style::default().fg(Color::Gray)));
                        }
                        history_spans.push(Span::styled(
                            format!("... ({} omitted)", total_count - 6),
                            Style::default().fg(Color::Gray),
                        ));
                    }

                    if let Some(fields) = &history_entry.fields {
                        if let Some(state_change) = &fields.state {
                            if let Some(new_state) = &state_change.new_value {
                                // Add arrow separator between entries (showing chronological flow)
                                if !history_spans.is_empty() {
                                    history_spans.push(Span::styled(
                                        " → ",
                                        Style::default().fg(Color::Gray),
                                    ));
                                }

                                // Format date - use System.ChangedDate as primary source
                                let date_str = {
                                    // First try System.ChangedDate
                                    if let Some(fields) = &history_entry.fields {
                                        if let Some(changed_date) = &fields.changed_date {
                                            if let Some(new_date) = &changed_date.new_value {
                                                if !new_date.starts_with("9999-01-01") {
                                                    // Extract date part from System.ChangedDate
                                                    if let Some(t_pos) = new_date.find('T') {
                                                        &new_date[..t_pos]
                                                    } else {
                                                        new_date
                                                    }
                                                } else {
                                                    "Unknown date"
                                                }
                                            } else {
                                                "Unknown date"
                                            }
                                        } else {
                                            // No System.ChangedDate, try revisedDate
                                            if !history_entry.revised_date.starts_with("9999-01-01")
                                            {
                                                if let Some(t_pos) =
                                                    history_entry.revised_date.find('T')
                                                {
                                                    &history_entry.revised_date[..t_pos]
                                                } else {
                                                    &history_entry.revised_date
                                                }
                                            } else {
                                                "Unknown date"
                                            }
                                        }
                                    } else {
                                        // No fields, try revisedDate
                                        if !history_entry.revised_date.starts_with("9999-01-01") {
                                            if let Some(t_pos) =
                                                history_entry.revised_date.find('T')
                                            {
                                                &history_entry.revised_date[..t_pos]
                                            } else {
                                                &history_entry.revised_date
                                            }
                                        } else {
                                            "Unknown date"
                                        }
                                    }
                                };

                                // Get color for the state
                                let state_color = get_state_color(new_state);

                                history_spans.push(Span::styled(
                                    "●",
                                    Style::default()
                                        .fg(state_color)
                                        .add_modifier(Modifier::BOLD),
                                ));
                                history_spans.push(Span::raw(" "));
                                history_spans.push(Span::styled(
                                    new_state.clone(),
                                    Style::default().fg(state_color),
                                ));
                                history_spans.push(Span::styled(
                                    format!(" ({})", date_str),
                                    Style::default().fg(Color::Gray),
                                ));
                            }
                        }
                    }
                }
            }
        }

        let history_line = Line::from(history_spans);
        let history_widget = Paragraph::new(vec![history_line])
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL).title("History"))
            .wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(history_widget, area);
    }
}

#[async_trait]
impl AppState for PullRequestSelectionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        // Initialize selection if not already set
        self.initialize_selection(app);

        // Handle empty PR list
        if app.pull_requests.is_empty() {
            let empty_message =
                Paragraph::new("No pull requests found without merged tags.\n\nPress 'q' to quit.")
                    .style(Style::default().fg(Color::Yellow))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("No Pull Requests"),
                    )
                    .alignment(Alignment::Center);
            f.render_widget(empty_message, f.area());
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Percentage(50), // Top half for PR table
                    Constraint::Percentage(40), // Bottom half for work item details
                    Constraint::Length(3),      // Help section
                ]
                .as_ref(),
            )
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

                // Apply background highlighting for selected items
                let row_style = if pr_with_wi.selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };

                let cells = vec![
                    Cell::from(selected).style(if pr_with_wi.selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    }),
                    Cell::from(format!("{:<6}", pr_with_wi.pr.id)) // Left-aligned with fixed width
                        .style(if pr_with_wi.selected {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::Cyan)
                        }),
                    Cell::from(date).style(if pr_with_wi.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default()
                    }),
                    Cell::from(pr_with_wi.pr.title.clone()).style(if pr_with_wi.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default()
                    }),
                    Cell::from(pr_with_wi.pr.created_by.display_name.clone()).style(
                        if pr_with_wi.selected {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::Yellow)
                        },
                    ),
                    Cell::from(work_items).style(if pr_with_wi.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(get_work_items_color(&pr_with_wi.work_items))
                    }),
                ];

                Row::new(cells).height(1).style(row_style)
            })
            .collect();

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(3),      // Selection checkbox
                Constraint::Length(8),      // PR # (fixed width)
                Constraint::Length(12),     // Date
                Constraint::Percentage(30), // Title
                Constraint::Percentage(20), // Author
                Constraint::Percentage(25), // Work Items
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

        // Render work item details
        self.render_work_item_details(f, app, chunks[1]);

        let help = List::new(vec![
            ListItem::new("↑/↓: Navigate PRs | ←/→: Navigate Work Items | Space: Toggle | Enter: Confirm | p: Open PR | w: Open Work Items | r: Refresh | q: Quit"),
        ])
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
            KeyCode::Left => {
                self.previous_work_item(app);
                StateChange::Keep
            }
            KeyCode::Right => {
                self.next_work_item(app);
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
                if let Some(pr_index) = self.table_state.selected() {
                    if let Some(pr) = app.pull_requests.get(pr_index) {
                        if !pr.work_items.is_empty() {
                            // Ensure work_item_index is within bounds
                            let work_item_index = if self.work_item_index < pr.work_items.len() {
                                self.work_item_index
                            } else {
                                0
                            };

                            if let Some(work_item) = pr.work_items.get(work_item_index) {
                                // Open only the currently displayed work item
                                app.open_work_items_in_browser(&[work_item.clone()]);
                            }
                        }
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
            KeyCode::Char('r') => {
                // Refresh: go back to data loading state to re-fetch PRs
                StateChange::Change(Box::new(DataLoadingState::new()))
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
