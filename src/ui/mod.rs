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

mod app;
mod state;

pub use app::App;

pub async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
) -> io::Result<Vec<usize>> {
    loop {
        terminal.draw(|f| app.draw(f))?;

        if let Event::Key(key) = event::read()? {
            if let Some(result) = app.process_key(key) {
                return Ok(result);
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
