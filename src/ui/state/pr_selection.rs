use crate::{
    models::PullRequestWithWorkItems,
    ui::App,
    ui::state::{AppState, StateChange, VersionInputState},
};
use async_trait::async_trait;
use chrono::DateTime;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

pub struct PullRequestSelectionState {
    list_state: ListState,
}

impl PullRequestSelectionState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self { list_state }
    }

    fn next(&mut self, app: &App) {
        if app.pull_requests.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= app.pull_requests.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous(&mut self, app: &App) {
        if app.pull_requests.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    app.pull_requests.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn toggle_selection(&mut self, app: &mut App) {
        if let Some(i) = self.list_state.selected() {
            if let Some(pr) = app.pull_requests.get_mut(i) {
                pr.selected = !pr.selected;
            }
        }
    }
}

#[async_trait]
impl AppState for PullRequestSelectionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(f.size());

        let items: Vec<ListItem> = app
            .pull_requests
            .iter()
            .map(|pr_with_wi| {
                let mut spans = vec![];

                if pr_with_wi.selected {
                    spans.push(Span::styled("[x] ", Style::default().fg(Color::Green)));
                } else {
                    spans.push(Span::raw("[ ] "));
                }

                spans.push(Span::styled(
                    format!("PR #{} ", pr_with_wi.pr.id),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));

                if let Ok(date) = DateTime::parse_from_rfc3339(&pr_with_wi.pr.creation_date) {
                    spans.push(Span::raw(format!("[{}] ", date.format("%Y-%m-%d"))));
                }

                if !pr_with_wi.work_items.is_empty() {
                    spans.push(Span::raw("WI: "));
                    for (i, wi) in pr_with_wi.work_items.iter().enumerate() {
                        if i > 0 {
                            spans.push(Span::raw(", "));
                        }
                        let state = wi.fields.state.as_deref().unwrap_or("Unknown");
                        let color = get_state_color(state);
                        spans.push(Span::styled(
                            format!("#{} ({})", wi.id, state),
                            Style::default().fg(color),
                        ));
                    }
                    spans.push(Span::raw(" | "));
                }

                spans.push(Span::raw(format!("{} ", pr_with_wi.pr.title)));
                spans.push(Span::styled(
                    format!("by {}", pr_with_wi.pr.created_by.display_name),
                    Style::default().fg(Color::Yellow),
                ));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Pull Requests"),
            )
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, chunks[0], &mut self.list_state);

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
                if let Some(i) = self.list_state.selected() {
                    if let Some(pr) = app.pull_requests.get(i) {
                        app.open_pr_in_browser(pr.pr.id);
                    }
                }
                StateChange::Keep
            }
            KeyCode::Char('w') => {
                if let Some(i) = self.list_state.selected() {
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
