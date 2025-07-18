use crate::ui::App;
use crate::ui::state::{AppState, StateChange};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationTab {
    Eligible,
    Unsure,
    NotMerged,
}

pub struct MigrationState {
    pub current_tab: MigrationTab,
    pub eligible_list_state: ListState,
    pub unsure_list_state: ListState,
    pub not_merged_list_state: ListState,
    pub show_details: bool,
}

impl MigrationState {
    pub fn new() -> Self {
        let mut eligible_list_state = ListState::default();
        eligible_list_state.select(Some(0));

        Self {
            current_tab: MigrationTab::Eligible,
            eligible_list_state,
            unsure_list_state: ListState::default(),
            not_merged_list_state: ListState::default(),
            show_details: false,
        }
    }

    fn get_current_list_state(&mut self) -> &mut ListState {
        match self.current_tab {
            MigrationTab::Eligible => &mut self.eligible_list_state,
            MigrationTab::Unsure => &mut self.unsure_list_state,
            MigrationTab::NotMerged => &mut self.not_merged_list_state,
        }
    }

    fn get_current_prs_count(&self, app: &App) -> usize {
        if let Some(analysis) = &app.migration_analysis {
            match self.current_tab {
                MigrationTab::Eligible => analysis.eligible_prs.len(),
                MigrationTab::Unsure => analysis.unsure_prs.len(),
                MigrationTab::NotMerged => analysis.not_merged_prs.len(),
            }
        } else {
            0
        }
    }

    fn move_selection(&mut self, app: &App, direction: i32) {
        let count = self.get_current_prs_count(app);

        if count == 0 {
            return;
        }

        let current_list = self.get_current_list_state();
        let current = current_list.selected().unwrap_or(0);
        let new_index = if direction > 0 {
            (current + 1) % count
        } else {
            if current == 0 { count - 1 } else { current - 1 }
        };
        current_list.select(Some(new_index));
    }

    fn switch_tab(&mut self, app: &App, direction: i32) {
        self.current_tab = match self.current_tab {
            MigrationTab::Eligible => {
                if direction > 0 {
                    MigrationTab::Unsure
                } else {
                    MigrationTab::NotMerged
                }
            }
            MigrationTab::Unsure => {
                if direction > 0 {
                    MigrationTab::NotMerged
                } else {
                    MigrationTab::Eligible
                }
            }
            MigrationTab::NotMerged => {
                if direction > 0 {
                    MigrationTab::Eligible
                } else {
                    MigrationTab::Unsure
                }
            }
        };

        // Ensure the new tab has a valid selection
        let count = self.get_current_prs_count(app);
        if count > 0 {
            let current_list = self.get_current_list_state();
            if current_list.selected().is_none() {
                current_list.select(Some(0));
            }
        }
    }

    fn get_current_pr<'a>(
        &self,
        app: &'a App,
    ) -> Option<&'a crate::models::PullRequestWithWorkItems> {
        if let Some(analysis) = &app.migration_analysis {
            let list_state = match self.current_tab {
                MigrationTab::Eligible => &self.eligible_list_state,
                MigrationTab::Unsure => &self.unsure_list_state,
                MigrationTab::NotMerged => &self.not_merged_list_state,
            };

            if let Some(selected) = list_state.selected() {
                match self.current_tab {
                    MigrationTab::Eligible => analysis.eligible_prs.get(selected),
                    MigrationTab::Unsure => analysis.unsure_prs.get(selected),
                    MigrationTab::NotMerged => analysis.not_merged_prs.get(selected),
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    fn open_current_pr(&self, app: &App) {
        if let Some(pr) = self.get_current_pr(app) {
            app.open_pr_in_browser(pr.pr.id);
        }
    }

    fn render_tabs(&self, f: &mut Frame, app: &App, area: Rect) {
        let analysis = app.migration_analysis.as_ref().unwrap();

        let tab_titles = vec![
            format!("✅ Eligible ({})", analysis.eligible_prs.len()),
            format!("❓ Unsure ({})", analysis.unsure_prs.len()),
            format!("❌ Not Merged ({})", analysis.not_merged_prs.len()),
        ];

        let tabs = Tabs::new(tab_titles)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(Color::Yellow).bold())
            .select(match self.current_tab {
                MigrationTab::Eligible => 0,
                MigrationTab::Unsure => 1,
                MigrationTab::NotMerged => 2,
            });

        f.render_widget(tabs, area);
    }

    fn render_pr_list(&mut self, f: &mut Frame, app: &App, area: Rect) {
        let analysis = app.migration_analysis.as_ref().unwrap();

        let (prs, title, color) = match self.current_tab {
            MigrationTab::Eligible => (
                &analysis.eligible_prs,
                "Eligible PRs - Ready for tagging",
                Color::Green,
            ),
            MigrationTab::Unsure => (
                &analysis.unsure_prs,
                "Unsure PRs - Require manual review",
                Color::Yellow,
            ),
            MigrationTab::NotMerged => (
                &analysis.not_merged_prs,
                "Not Merged PRs - Not ready for migration",
                Color::Red,
            ),
        };

        let items: Vec<ListItem> = prs
            .iter()
            .map(|pr| {
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!("#{}", pr.pr.id),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::raw(&pr.pr.title),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("  By: {}", pr.pr.created_by.display_name),
                            Style::default().fg(Color::Gray),
                        ),
                        Span::raw(" | "),
                        Span::styled(
                            format!("Work Items: {}", pr.work_items.len()),
                            Style::default().fg(Color::Gray),
                        ),
                    ]),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(color)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let current_list = self.get_current_list_state();
        f.render_stateful_widget(list, area, current_list);
    }

    fn render_details(&self, f: &mut Frame, app: &App, area: Rect) {
        if let Some(pr) = self.get_current_pr(app) {
            let analysis = app.migration_analysis.as_ref().unwrap();

            let mut details = vec![
                Line::from(vec![Span::styled(
                    "PR Details:",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![Span::raw(format!("ID: #{}", pr.pr.id))]),
                Line::from(vec![Span::raw(format!("Title: {}", pr.pr.title))]),
                Line::from(vec![Span::raw(format!(
                    "Created By: {}",
                    pr.pr.created_by.display_name
                ))]),
                Line::from(""),
            ];

            // Add work items information
            if pr.work_items.is_empty() {
                details.push(Line::from(vec![Span::styled(
                    "Work Items: None",
                    Style::default().fg(Color::Gray),
                )]));
            } else {
                details.push(Line::from(vec![Span::styled(
                    "Work Items:",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]));
                for work_item in &pr.work_items {
                    let state = work_item.fields.state.as_deref().unwrap_or("Unknown");
                    let color = if analysis.terminal_states.contains(&state.to_string()) {
                        Color::Green
                    } else {
                        Color::Red
                    };
                    details.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            format!("#{}", work_item.id),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(" - "),
                        Span::raw(work_item.fields.title.as_deref().unwrap_or("No title")),
                        Span::raw(" ("),
                        Span::styled(state, Style::default().fg(color)),
                        Span::raw(")"),
                    ]));
                }
            }

            // Add general reason for all PRs using all_details
            if let Some(detail) = analysis.all_details.iter().find(|d| d.pr.pr.id == pr.pr.id) {
                if let Some(reason) = &detail.reason {
                    details.push(Line::from(""));
                    details.push(Line::from(vec![Span::styled(
                        "Reason:",
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    )]));
                    details.push(Line::from(vec![Span::raw(reason)]));
                }
            }

            // Add unsure reason for unsure PRs (legacy support)
            if self.current_tab == MigrationTab::Unsure {
                if let Some(unsure_detail) = analysis
                    .unsure_details
                    .iter()
                    .find(|d| d.pr.pr.id == pr.pr.id)
                {
                    if let Some(reason) = &unsure_detail.unsure_reason {
                        details.push(Line::from(""));
                        details.push(Line::from(vec![Span::styled(
                            "Unsure Reason:",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        )]));
                        details.push(Line::from(vec![Span::raw(reason)]));
                    }
                }
            }

            let paragraph = Paragraph::new(details)
                .block(Block::default().borders(Borders::ALL).title("Details"))
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, area);
        }
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help_text = vec![
            Line::from(vec![Span::styled(
                "Navigation:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from("  ↑/↓ - Navigate PRs"),
            Line::from("  ←/→ - Switch tabs"),
            Line::from("  Enter/Space - Open PR in browser"),
            Line::from("  d - Toggle details"),
            Line::from("  q - Quit"),
        ];

        let paragraph = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}

#[async_trait]
impl AppState for MigrationState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        if app.migration_analysis.is_none() {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tabs
                Constraint::Min(10),   // Main content
                Constraint::Length(8), // Help
            ])
            .split(f.area());

        // Render tabs
        self.render_tabs(f, app, chunks[0]);

        // Split main content area
        let main_chunks = if self.show_details {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[1])
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(chunks[1])
        };

        // Render PR list
        self.render_pr_list(f, app, main_chunks[0]);

        // Render details if enabled
        if self.show_details && main_chunks.len() > 1 {
            self.render_details(f, app, main_chunks[1]);
        }

        // Render help
        self.render_help(f, chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Up => {
                self.move_selection(app, -1);
                StateChange::Keep
            }
            KeyCode::Down => {
                self.move_selection(app, 1);
                StateChange::Keep
            }
            KeyCode::Left => {
                self.switch_tab(app, -1);
                StateChange::Keep
            }
            KeyCode::Right => {
                self.switch_tab(app, 1);
                StateChange::Keep
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.open_current_pr(app);
                StateChange::Keep
            }
            KeyCode::Char('d') => {
                self.show_details = !self.show_details;
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }
}
