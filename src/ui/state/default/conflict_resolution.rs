use crate::{
    git,
    ui::App,
    ui::state::{AppState, CherryPickContinueState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub struct ConflictResolutionState {
    conflicted_files: Vec<String>,
}

impl ConflictResolutionState {
    pub fn new(conflicted_files: Vec<String>) -> Self {
        Self { conflicted_files }
    }

    fn render_commit_info(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        current_item: &crate::models::CherryPickItem,
        app: &crate::ui::App,
    ) {
        let mut commit_text = vec![];

        // Try to get detailed commit info from git
        if let Some(repo_path) = &app.repo_path {
            match crate::git::get_commit_info(repo_path, &current_item.commit_id) {
                Ok(commit_info) => {
                    // Shortened commit hash
                    let short_hash = if commit_info.hash.len() >= 8 {
                        commit_info.hash[..8].to_string()
                    } else {
                        commit_info.hash.clone()
                    };

                    commit_text.push(Line::from(vec![
                        Span::raw("Hash: "),
                        Span::styled(short_hash, Style::default().fg(Color::Yellow)),
                    ]));

                    // Format and display date
                    let date_part = if let Some(space_pos) = commit_info.date.find(' ') {
                        commit_info.date[..space_pos].to_string()
                    } else {
                        commit_info.date.clone()
                    };
                    commit_text.push(Line::from(vec![
                        Span::raw("Date: "),
                        Span::styled(date_part, Style::default().fg(Color::Gray)),
                    ]));

                    commit_text.push(Line::from(vec![
                        Span::raw("Author: "),
                        Span::styled(commit_info.author, Style::default().fg(Color::Green)),
                    ]));

                    commit_text.push(Line::from(""));
                    commit_text.push(Line::from(vec![
                        Span::raw("Title: "),
                        Span::raw(commit_info.title),
                    ]));
                }
                Err(_) => {
                    // Fallback to basic info if git command fails
                    let short_hash = if current_item.commit_id.len() >= 8 {
                        &current_item.commit_id[..8]
                    } else {
                        &current_item.commit_id
                    };

                    commit_text.push(Line::from(vec![
                        Span::raw("Hash: "),
                        Span::styled(short_hash, Style::default().fg(Color::Yellow)),
                    ]));

                    commit_text.push(Line::from(vec![
                        Span::raw("Title: "),
                        Span::raw(&current_item.pr_title),
                    ]));
                }
            }
        } else {
            // Fallback if no repo path available
            let short_hash = if current_item.commit_id.len() >= 8 {
                &current_item.commit_id[..8]
            } else {
                &current_item.commit_id
            };

            commit_text.push(Line::from(vec![
                Span::raw("Hash: "),
                Span::styled(short_hash, Style::default().fg(Color::Yellow)),
            ]));

            commit_text.push(Line::from(vec![
                Span::raw("Title: "),
                Span::raw(&current_item.pr_title),
            ]));
        }

        let commit_widget = Paragraph::new(commit_text)
            .block(Block::default().borders(Borders::ALL).title("Commit"))
            .wrap(Wrap { trim: true });
        f.render_widget(commit_widget, area);
    }

    fn render_conflicted_files(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let files: Vec<ListItem> = self
            .conflicted_files
            .iter()
            .map(|file| ListItem::new(format!("  • {}", file)))
            .collect();

        let file_list = List::new(files)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Conflicted Files"),
            )
            .style(Style::default().fg(Color::Red));
        f.render_widget(file_list, area);
    }

    fn render_pr_details(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        pr: Option<&crate::models::PullRequest>,
    ) {
        let mut pr_text = vec![];

        if let Some(pr) = pr {
            pr_text.push(Line::from(vec![
                Span::raw("PR #"),
                Span::styled(
                    format!("{}", pr.id),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            if let Some(date) = &pr.closed_date {
                pr_text.push(Line::from(vec![
                    Span::raw("Date: "),
                    Span::styled(date, Style::default().fg(Color::Gray)),
                ]));
            }

            pr_text.push(Line::from(vec![
                Span::raw("Author: "),
                Span::styled(
                    &pr.created_by.display_name,
                    Style::default().fg(Color::Green),
                ),
            ]));

            pr_text.push(Line::from(""));
            pr_text.push(Line::from(vec![Span::raw("Title: "), Span::raw(&pr.title)]));
        } else {
            pr_text.push(Line::from("PR details not found"));
        }

        let pr_widget = Paragraph::new(pr_text)
            .block(Block::default().borders(Borders::ALL).title("Pull Request"))
            .wrap(Wrap { trim: true });
        f.render_widget(pr_widget, area);
    }

    fn render_work_item_details(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        work_items: &[crate::models::WorkItem],
    ) {
        let mut wi_text = vec![];

        if work_items.is_empty() {
            wi_text.push(Line::from("No work items linked"));
        } else {
            for (i, wi) in work_items.iter().enumerate() {
                if i > 0 {
                    wi_text.push(Line::from(""));
                    wi_text.push(Line::from("─────────────────"));
                    wi_text.push(Line::from(""));
                }

                wi_text.push(Line::from(vec![
                    Span::styled(
                        wi.fields.work_item_type.as_deref().unwrap_or("Item"),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" #"),
                    Span::styled(format!("{}", wi.id), Style::default().fg(Color::Cyan)),
                ]));

                if let Some(state) = &wi.fields.state {
                    wi_text.push(Line::from(vec![
                        Span::raw("State: "),
                        Span::styled(state, Style::default().fg(Color::Yellow)),
                    ]));
                }

                if let Some(assigned_to) = &wi.fields.assigned_to {
                    wi_text.push(Line::from(vec![
                        Span::raw("Assigned: "),
                        Span::styled(&assigned_to.display_name, Style::default().fg(Color::Green)),
                    ]));
                }

                if let Some(title) = &wi.fields.title {
                    wi_text.push(Line::from(""));
                    wi_text.push(Line::from(vec![Span::raw("Title: "), Span::raw(title)]));
                }
            }
        }

        let wi_widget = Paragraph::new(wi_text)
            .block(Block::default().borders(Borders::ALL).title("Work Items"))
            .wrap(Wrap { trim: true });
        f.render_widget(wi_widget, area);
    }
}

#[async_trait]
impl AppState for ConflictResolutionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        // Main layout: Title at top, content in middle, help at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(0),    // Content
                Constraint::Length(5), // Instructions + Help
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("⚠️  Merge Conflict Detected")
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Split content horizontally: Left and Right panes
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);

        // Left pane: Split vertically for commit info (20%) and conflicted files (80%)
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(content_chunks[0]);

        // Right pane: Split vertically for PR details and work item details
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(content_chunks[1]);

        // Get current cherry-pick item
        let current_item = &app.cherry_pick_items[app.current_cherry_pick_index];

        // Find the corresponding PR and work items
        let pr_with_work_items = app
            .pull_requests
            .iter()
            .find(|pr| pr.pr.id == current_item.pr_id);

        // Top-left: Commit Information (20%)
        self.render_commit_info(f, left_chunks[0], current_item, app);

        // Bottom-left: Conflicted Files (80%)
        self.render_conflicted_files(f, left_chunks[1]);

        // Top-right: PR Details
        self.render_pr_details(f, right_chunks[0], pr_with_work_items.map(|p| &p.pr));

        // Bottom-right: Work Item Details
        let work_items = pr_with_work_items
            .map(|p| p.work_items.as_slice())
            .unwrap_or(&[]);
        self.render_work_item_details(f, right_chunks[1], work_items);

        // Bottom: Instructions and Help
        let repo_path = app.repo_path.as_ref().unwrap().display();
        let instructions = vec![
            Line::from(vec![
                Span::raw("Repository: "),
                Span::styled(format!("{}", repo_path), Style::default().fg(Color::Cyan)),
            ]),
            Line::from("Please resolve conflicts in another terminal and stage the changes."),
            Line::from(vec![Span::styled(
                "c: Continue (after resolving) | a: Abort",
                Style::default().fg(Color::Gray),
            )]),
        ];

        let instructions_widget = Paragraph::new(instructions)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .style(Style::default().fg(Color::White));
        f.render_widget(instructions_widget, main_chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        let repo_path = app.repo_path.as_ref().unwrap();

        match code {
            KeyCode::Char('c') => {
                // Check if conflicts are resolved
                match git::check_conflicts_resolved(repo_path) {
                    Ok(true) => {
                        // Transition to CherryPickContinueState to process the commit with feedback
                        StateChange::Change(Box::new(CherryPickContinueState::new(
                            self.conflicted_files.clone(),
                            repo_path.clone(),
                        )))
                    }
                    Ok(false) => StateChange::Keep, // Conflicts not resolved
                    Err(_) => StateChange::Keep,
                }
            }
            KeyCode::Char('a') => {
                // Abort entire process
                let _ = git::abort_cherry_pick(repo_path);
                StateChange::Change(Box::new(super::CompletionState::new()))
            }
            _ => StateChange::Keep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{CherryPickItem, CherryPickStatus},
        ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_default},
        },
    };
    use insta::assert_snapshot;
    use std::path::PathBuf;

    /// # Conflict Resolution State - Display
    ///
    /// Tests the conflict resolution screen.
    ///
    /// ## Test Scenario
    /// - Creates a conflict resolution state
    /// - Sets up a conflicted PR
    /// - Renders the conflict resolution screen
    ///
    /// ## Expected Outcome
    /// - Should display conflict warning
    /// - Should show conflicted PR details
    /// - Should display resolution options
    #[test]
    fn test_conflict_resolution_display() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            harness.app.cherry_pick_items = vec![CherryPickItem {
                commit_id: "abc123".to_string(),
                pr_id: 100,
                pr_title: "Fix critical database migration bug".to_string(),
                status: CherryPickStatus::Conflict,
            }];
            harness.app.repo_path = Some(PathBuf::from("/path/to/repo"));
            harness.app.current_cherry_pick_index = 0;

            let conflicted_files = vec!["src/database/migrations.rs".to_string()];
            let state = Box::new(ConflictResolutionState::new(conflicted_files));
            harness.render_state(state);

            assert_snapshot!("conflict_display", harness.backend());
        });
    }
}
