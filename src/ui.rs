use chrono::DateTime;
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::{io, process::Command};

use crate::models::{PullRequestWithWorkItems, WorkItem};

pub struct App {
    pub pull_requests: Vec<PullRequestWithWorkItems>,
    pub state: ListState,
    pub organization: String,
    pub project: String,
    pub repository: String,
}

impl App {
    pub fn new(
        pull_requests: Vec<PullRequestWithWorkItems>,
        organization: String,
        project: String,
        repository: String,
    ) -> Self {
        let mut state = ListState::default();
        if !pull_requests.is_empty() {
            state.select(Some(0));
        }
        Self {
            pull_requests,
            state,
            organization,
            project,
            repository,
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

    fn open_pr_in_browser(&self) {
        if let Some(i) = self.state.selected() {
            if let Some(pr) = self.pull_requests.get(i) {
                let url = format!(
                    "https://dev.azure.com/{}/{}/_git/{}/pullrequest/{}",
                    self.organization, self.project, self.repository, pr.pr.id
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

    fn open_work_items_in_browser(&self) {
        if let Some(i) = self.state.selected() {
            if let Some(pr) = self.pull_requests.get(i) {
                for wi in &pr.work_items {
                    let url = format!(
                        "https://dev.azure.com/{}/{}/_workitems/edit/{}",
                        self.organization, self.project, wi.id
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

fn ui(f: &mut Frame, app: &App) {
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

            // Selection indicator
            if pr_with_wi.selected {
                spans.push(Span::styled("[x] ", Style::default().fg(Color::Green)));
            } else {
                spans.push(Span::raw("[ ] "));
            }

            // PR number
            spans.push(Span::styled(
                format!("PR #{} ", pr_with_wi.pr.id),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

            // Date
            if let Ok(date) = DateTime::parse_from_rfc3339(&pr_with_wi.pr.creation_date) {
                spans.push(Span::raw(format!("[{}] ", date.format("%Y-%m-%d"))));
            }

            // Work items with states
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

            // Title
            spans.push(Span::raw(format!("{} ", pr_with_wi.pr.title)));

            // Author
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

    f.render_stateful_widget(list, chunks[0], &mut app.state.clone());

    let help = List::new(vec![ListItem::new(
        "↑/↓: Navigate | Space: Toggle selection | Enter: Confirm | p: Open PR | w: Open Work Items | q: Quit",
    )])
    .block(Block::default().borders(Borders::ALL).title("Help"));

    f.render_widget(help, chunks[1]);
}

pub async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
) -> io::Result<Vec<usize>> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(vec![]),
                KeyCode::Up => app.previous(),
                KeyCode::Down => app.next(),
                KeyCode::Char(' ') => app.toggle_selection(),
                KeyCode::Char('p') => app.open_pr_in_browser(),
                KeyCode::Char('w') => app.open_work_items_in_browser(),
                KeyCode::Enter => {
                    let selected_indices: Vec<usize> = app
                        .pull_requests
                        .iter()
                        .enumerate()
                        .filter(|(_, pr)| pr.selected)
                        .map(|(i, _)| i)
                        .collect();
                    return Ok(selected_indices);
                }
                _ => {}
            }
        }
    }
}

pub fn print_pr_links(
    selected_prs: &[&PullRequestWithWorkItems],
    org: &str,
    project: &str,
    repo: &str,
) {
    println!("\nPR Links:");
    for pr in selected_prs {
        let url = format!(
            "https://dev.azure.com/{}/{}/_git/{}/pullrequest/{}",
            org, project, repo, pr.pr.id
        );
        println!("  \x1b]8;;{}\x1b\\PR #{}\x1b]8;;\x1b\\", url, pr.pr.id);
    }
}

pub fn print_work_item_links(selected_prs: &[&PullRequestWithWorkItems], org: &str, project: &str) {
    use colored::Colorize;

    println!("\nWork Item Links:");
    for pr in selected_prs {
        for wi in &pr.work_items {
            let url = format!(
                "https://dev.azure.com/{}/{}/_workitems/edit/{}",
                org, project, wi.id
            );
            let state = wi.fields.state.as_deref().unwrap_or("Unknown");
            let title = wi.fields.title.as_deref().unwrap_or("No title");

            // Use colored crate for terminal output
            let colored_text = match state {
                "Dev Closed" => format!("WI #{}: {}", wi.id, title).bright_green(),
                "Closed" => format!("WI #{}: {}", wi.id, title).green(),
                "Resolved" => format!("WI #{}: {}", wi.id, title).truecolor(255, 165, 0), // Orange
                "In Review" => format!("WI #{}: {}", wi.id, title).yellow(),
                "New" => format!("WI #{}: {}", wi.id, title).bright_black(),
                "Active" => format!("WI #{}: {}", wi.id, title).blue(),
                "Next Merged" => format!("WI #{}: {}", wi.id, title).red(),
                "Next Closed" => format!("WI #{}: {}", wi.id, title).purple(),
                "Hold" => format!("WI #{}: {}", wi.id, title).cyan(),
                _ => format!("WI #{}: {}", wi.id, title).white(),
            };

            // Print with clickable link
            println!("  \x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, colored_text);
        }
    }
}
