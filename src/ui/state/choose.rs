use std::process::Command;

use chrono::DateTime;
use crossterm::event::KeyCode;
use crossterm::event::{self, Event};
use ratatui::style::Stylize;
use ratatui::text::Text;
use ratatui::widgets::{Cell, Row, Table, TableState};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::io;

use crate::{models::PullRequestWithWorkItems, ui::app::App};

use super::{AppState, InitialState, StateChange};

struct LoadingState {}

struct ReadyState {}

pub struct ChooseState {
    pull_requests: Vec<PullRequestWithWorkItems>,
    state: TableState,
}

impl ChooseState {
    pub fn new(pull_requests: Vec<PullRequestWithWorkItems>) -> Self {
        let mut state = TableState::default();
        if !pull_requests.is_empty() {
            state.select(Some(0));
        }
        Self {
            pull_requests,
            state,
        }
    }
    fn next(&mut self) {
        if self.pull_requests.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.pull_requests.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.pull_requests.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.pull_requests.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn toggle_selection(&mut self) {
        if let Some(i) = self.state.selected() {
            if let Some(pr) = self.pull_requests.get_mut(i) {
                pr.selected = !pr.selected;
            }
        }
    }

    fn open_pr_in_browser(&self, app: &App) {
        if let Some(i) = self.state.selected() {
            if let Some(pr) = self.pull_requests.get(i) {
                let url = format!(
                    "https://dev.azure.com/{}/{}/_git/{}/pullrequest/{}",
                    app.organization, app.project, app.repository, pr.pr.id
                );

                #[cfg(target_os = "macos")]
                let _ = Command::new("open").arg(&url).spawn();

                #[cfg(target_os = "linux")]
                let _ = Command::new("xdg-open").arg(&url).spawn();

                #[cfg(target_os = "windows")]
                let _ = Command::new("cmd").args(&["/C", "start", &url]).spawn();
            }
        }
    }

    fn open_work_items_in_browser(&self, app: &App) {
        if let Some(i) = self.state.selected() {
            if let Some(pr) = self.pull_requests.get(i) {
                for wi in &pr.work_items {
                    let url = format!(
                        "https://dev.azure.com/{}/{}/_workitems/edit/{}",
                        app.organization, app.project, wi.id
                    );

                    #[cfg(target_os = "macos")]
                    let _ = Command::new("open").arg(&url).spawn();

                    #[cfg(target_os = "linux")]
                    let _ = Command::new("xdg-open").arg(&url).spawn();

                    #[cfg(target_os = "windows")]
                    let _ = Command::new("cmd").args(&["/C", "start", &url]).spawn();
                }
            }
        }
    }
}

fn get_state_color(state: &str) -> Color {
    match state {
        "Dev Closed" => Color::LightGreen,
        "Closed" => Color::Green,
        "Resolved" => Color::Rgb(255, 165, 0), // Orange
        "In Review" => Color::Yellow,
        "New" => Color::Gray,
        "Active" => Color::Blue,
        "Next Merged" => Color::Red,
        "Next Closed" => Color::Magenta,
        "Hold" => Color::Cyan,
        _ => Color::White,
    }
}

static PR_TITLES: &[&str] = &["X", "PR #", "Date", "Work Item #", "PR Title", "Author"];

static PR_WIDTHS: &[Constraint] = &[
    Constraint::Length(5),  // Selection
    Constraint::Length(15), // PR Number
    Constraint::Length(20), // Date
    Constraint::Length(15), // Work Item Number
    Constraint::Fill(1),    // PR Title
    Constraint::Length(20), // Author
];

fn pr_spans(pr: &PullRequestWithWorkItems) -> impl Iterator<Item = Cell> {
    let mut spans = vec![];

    // Selection indicator
    if pr.selected {
        spans.push(Span::styled("[x]", Style::default().fg(Color::Green)));
    } else {
        spans.push(Span::raw("[ ]"));
    }

    // PR number
    spans.push(Span::styled(
        format!("PR #{} ", pr.pr.id),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    // Date
    let date = DateTime::parse_from_rfc3339(&pr.pr.closed_date).unwrap();
    spans.push(Span::raw(format!("[{}] ", date.format("%Y-%m-%d"))));

    // Work items with states
    if !pr.work_items.is_empty() {

        pr.work_items.iter().map(|wi| )

        spans.push(Span::raw("WI: "));
        for (i, wi) in pr.work_items.iter().enumerate() {
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

    // Title
    spans.push(Span::raw(format!("{} ", pr.pr.title)));

    // Author
    spans.push(Span::styled(
        format!("by {}", pr.pr.created_by.display_name),
        Style::default().fg(Color::Yellow),
    ));
    spans.into_iter().map(Into::into)
}

fn pr_row(pr: &PullRequestWithWorkItems) -> Row {
    let spans = pr_spans(pr).into_iter().map(Cell::from);
    Row::new(spans)
}

fn pr_item(pr: &PullRequestWithWorkItems) -> ListItem {
    let spans = pr_spans(pr);
    ListItem::new(Line::from(spans))
}

impl InitialState for ChooseState {}

impl AppState for ChooseState {
    fn ui(&mut self, f: &mut ratatui::Frame, app: &crate::ui::app::App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(0)].as_ref())
            .split(f.area());

        let items: Vec<ListItem> = self.pull_requests.iter().map(pr_item).collect();

        let rows = self.pull_requests.iter().map(pr_row);
        let help = Row::new(vec![
            "HELP",
            "↑/↓: Navigate | Space: Toggle selection | Enter: Confirm | p: Open PR | w: Open Work Items | q: Quit",
        ]);
        let table = Table::new(rows, PR_WIDTHS)
            .column_spacing(1)
            // You can set the style of the entire Table.
            .style(Style::new().blue())
            // It has an optional header, which is simply a Row always visible at the top.
            .header(
                Row::new(PR_TITLES.iter().cloned())
                    .style(Style::new().bold())
                    // To add space between the header and the rest of the rows, specify the margin
                    .bottom_margin(1),
            )
            // It has an optional footer, which is simply a Row always visible at the bottom.
            .footer(help)
            // As any other widget, a Table can be wrapped in a Block.
            .block(Block::new().title("Pull Requests"))
            // The selected row, column, cell and its content can also be styled.
            .row_highlight_style(Style::new().reversed())
            .column_highlight_style(Style::new().red())
            .cell_highlight_style(Style::new().blue())
            // ...and potentially show a symbol in front of the selection.
            .highlight_symbol(">>");

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Pull Requests"),
            )
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("> ");

        f.render_stateful_widget(table, chunks[0], &mut self.state.clone());
    }

    fn process_key(&mut self, code: crossterm::event::KeyCode, app: &App) -> StateChange {
        match code {
            KeyCode::Char('q') => return StateChange::Exit,
            KeyCode::Up => self.previous(),
            KeyCode::Down => self.next(),
            KeyCode::Char(' ') => self.toggle_selection(),
            KeyCode::Char('p') => self.open_pr_in_browser(app),
            KeyCode::Char('w') => self.open_work_items_in_browser(app),
            KeyCode::Enter => {
                let selected_indices: Vec<usize> = self
                    .pull_requests
                    .iter()
                    .enumerate()
                    .filter(|(_, pr)| pr.selected)
                    .map(|(i, _)| i)
                    .collect();
                return StateChange::Exit;
            }
            _ => {}
        };
        StateChange::Keep
    }
}
