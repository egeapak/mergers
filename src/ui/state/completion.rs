use crate::{
    models::CherryPickStatus,
    ui::App,
    ui::state::{AppState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

pub struct CompletionState {
    list_state: ListState,
}

impl CompletionState {
    pub fn new() -> Self {
        let mut state = Self {
            list_state: ListState::default(),
        };
        state.list_state.select(Some(0));
        state
    }

    fn next(&mut self, app: &App) {
        if app.cherry_pick_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= app.cherry_pick_items.len() - 1 {
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
        if app.cherry_pick_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    app.cherry_pick_items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }
}

#[async_trait]
impl AppState for CompletionState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(f.area());

        let title = Paragraph::new("🏁 Cherry-pick Process Completed!")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Split the main area horizontally
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(main_chunks[1]);

        // Left side: Commit status list
        let available_width = content_chunks[0].width.saturating_sub(4); // Account for borders
        
        let items: Vec<ListItem> = app
            .cherry_pick_items
            .iter()
            .map(|item| {
                let mut spans = vec![];

                let (symbol, color) = match &item.status {
                    CherryPickStatus::Success => ("✅", Color::Green),
                    CherryPickStatus::Failed(_) => ("❌", Color::Red),
                    CherryPickStatus::Skipped => ("⏭️", Color::Gray),
                    CherryPickStatus::Conflict => ("⚠️", Color::Yellow),
                    _ => ("❓", Color::White),
                };

                spans.push(Span::styled(
                    format!("{} ", symbol),
                    Style::default().fg(color),
                ));

                let pr_prefix = format!("PR #{}: ", item.pr_id);
                spans.push(Span::styled(
                    pr_prefix.clone(),
                    Style::default().fg(Color::Cyan),
                ));

                // Find work items for this PR
                let work_items: Vec<i32> = app
                    .pull_requests
                    .iter()
                    .find(|pr| pr.pr.id == item.pr_id)
                    .map(|pr| pr.work_items.iter().map(|wi| wi.id).collect())
                    .unwrap_or_default();

                let work_items_text = if work_items.is_empty() {
                    String::new()
                } else {
                    format!(" [WI: {}]", work_items.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", "))
                };

                // Calculate available space for title
                let used_space = 3 + pr_prefix.len() + work_items_text.len(); // symbol + space + pr_prefix + work_items
                let title_space = if available_width as usize > used_space {
                    available_width as usize - used_space
                } else {
                    20 // minimum space
                };

                // Truncate title if needed to fit available space
                let title = if item.pr_title.len() > title_space {
                    if title_space > 3 {
                        format!("{}...", &item.pr_title[..title_space.saturating_sub(3)])
                    } else {
                        "...".to_string()
                    }
                } else {
                    item.pr_title.clone()
                };
                spans.push(Span::raw(title));

                if !work_items.is_empty() {
                    spans.push(Span::styled(
                        work_items_text,
                        Style::default().fg(Color::Magenta),
                    ));
                }

                if let CherryPickStatus::Failed(msg) = &item.status {
                    let max_error_len = (available_width as usize).saturating_sub(used_space + item.pr_title.len() + 3);
                    let error_text = if msg.len() > max_error_len && max_error_len > 3 {
                        format!(" - {}...", &msg[..max_error_len.saturating_sub(6)])
                    } else if max_error_len > 0 {
                        format!(" - {}", msg)
                    } else {
                        String::new()
                    };
                    if !error_text.is_empty() {
                        spans.push(Span::styled(
                            error_text,
                            Style::default().fg(Color::Red),
                        ));
                    }
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Cherry-pick Results"))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .fg(Color::Yellow),
            );
        f.render_stateful_widget(list, content_chunks[0], &mut self.list_state);

        // Right side: Summary and info
        let mut summary_text = vec![];

        // Calculate summary
        let mut successful = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for item in &app.cherry_pick_items {
            match &item.status {
                CherryPickStatus::Success => successful += 1,
                CherryPickStatus::Failed(_) => failed += 1,
                CherryPickStatus::Skipped => skipped += 1,
                _ => {}
            }
        }

        summary_text.push(Line::from(vec![
            Span::styled("Summary", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(""));
        summary_text.push(Line::from(vec![
            Span::raw("✅ Successful: "),
            Span::styled(format!("{}", successful), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(vec![
            Span::raw("❌ Failed: "),
            Span::styled(format!("{}", failed), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(vec![
            Span::raw("⏭️  Skipped: "),
            Span::styled(format!("{}", skipped), Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD)),
        ]));
        
        summary_text.push(Line::from(""));
        summary_text.push(Line::from("─────────────────────"));
        summary_text.push(Line::from(""));
        
        summary_text.push(Line::from(vec![
            Span::styled("Branch Info", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(""));
        
        let branch_name = format!(
            "patch/{}-{}",
            app.target_branch,
            app.version.as_ref().unwrap()
        );
        summary_text.push(Line::from(vec![
            Span::raw("Branch: "),
            Span::styled(branch_name, Style::default().fg(Color::Cyan)),
        ]));
        
        if let Some(repo_path) = &app.repo_path {
            summary_text.push(Line::from(vec![
                Span::raw("Location: "),
                Span::styled(
                    format!("{}", repo_path.display()),
                    Style::default().fg(Color::Blue),
                ),
            ]));
        }
        
        summary_text.push(Line::from(""));
        summary_text.push(Line::from("─────────────────────"));
        summary_text.push(Line::from(""));
        
        summary_text.push(Line::from(vec![
            Span::styled("Actions", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        summary_text.push(Line::from(""));
        summary_text.push(Line::from("↑/↓ Navigate"));
        summary_text.push(Line::from("'p' Open PR in browser"));
        summary_text.push(Line::from("'w' Open work items"));
        summary_text.push(Line::from(format!("'t' Tag PRs & update work items to '{}'", app.work_item_state)));
        summary_text.push(Line::from("'q' Exit"));

        let summary = Paragraph::new(summary_text)
            .block(Block::default().borders(Borders::ALL).title("Summary & Info"))
            .wrap(Wrap { trim: true });
        f.render_widget(summary, content_chunks[1]);
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
            KeyCode::Char('p') => {
                if let Some(i) = self.list_state.selected() {
                    if let Some(item) = app.cherry_pick_items.get(i) {
                        app.open_pr_in_browser(item.pr_id);
                    }
                }
                StateChange::Keep
            }
            KeyCode::Char('w') => {
                if let Some(i) = self.list_state.selected() {
                    if let Some(item) = app.cherry_pick_items.get(i) {
                        // Find the corresponding PR and open its work items
                        if let Some(pr) = app.pull_requests.iter().find(|pr| pr.pr.id == item.pr_id) {
                            if !pr.work_items.is_empty() {
                                app.open_work_items_in_browser(&pr.work_items);
                            }
                        }
                    }
                }
                StateChange::Keep
            }
            KeyCode::Char('t') => {
                StateChange::Change(Box::new(crate::ui::state::PostCompletionState::new()))
            }
            _ => StateChange::Keep,
        }
    }
}