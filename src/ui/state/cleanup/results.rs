use crate::{
    models::CleanupStatus,
    ui::App,
    ui::state::{AppState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs},
};

#[derive(Debug, Clone, PartialEq)]
enum ResultTab {
    Success,
    Failed,
}

pub struct CleanupResultsState {
    current_tab: ResultTab,
    success_list_state: ListState,
    failed_list_state: ListState,
}

impl Default for CleanupResultsState {
    fn default() -> Self {
        Self::new()
    }
}

impl CleanupResultsState {
    pub fn new() -> Self {
        let mut success_list_state = ListState::default();
        success_list_state.select(Some(0));

        let mut failed_list_state = ListState::default();
        failed_list_state.select(Some(0));

        Self {
            current_tab: ResultTab::Success,
            success_list_state,
            failed_list_state,
        }
    }

    fn switch_tab(&mut self) {
        self.current_tab = match self.current_tab {
            ResultTab::Success => ResultTab::Failed,
            ResultTab::Failed => ResultTab::Success,
        };
    }

    fn get_success_branches<'a>(&self, app: &'a App) -> Vec<&'a crate::models::CleanupBranch> {
        app.cleanup_branches
            .iter()
            .filter(|b| b.selected && matches!(b.status, CleanupStatus::Success))
            .collect()
    }

    fn get_failed_branches<'a>(&self, app: &'a App) -> Vec<&'a crate::models::CleanupBranch> {
        app.cleanup_branches
            .iter()
            .filter(|b| b.selected && matches!(b.status, CleanupStatus::Failed(_)))
            .collect()
    }

    fn next(&mut self, app: &App) {
        let count = match self.current_tab {
            ResultTab::Success => self.get_success_branches(app).len(),
            ResultTab::Failed => self.get_failed_branches(app).len(),
        };

        if count == 0 {
            return;
        }

        let list_state = match self.current_tab {
            ResultTab::Success => &mut self.success_list_state,
            ResultTab::Failed => &mut self.failed_list_state,
        };

        let i = match list_state.selected() {
            Some(i) => {
                if i >= count - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        list_state.select(Some(i));
    }

    fn previous(&mut self, app: &App) {
        let count = match self.current_tab {
            ResultTab::Success => self.get_success_branches(app).len(),
            ResultTab::Failed => self.get_failed_branches(app).len(),
        };

        if count == 0 {
            return;
        }

        let list_state = match self.current_tab {
            ResultTab::Success => &mut self.success_list_state,
            ResultTab::Failed => &mut self.failed_list_state,
        };

        let i = match list_state.selected() {
            Some(i) => {
                if i == 0 {
                    count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        list_state.select(Some(i));
    }
}

#[async_trait]
impl AppState for CleanupResultsState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("Cleanup Mode - Results")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Tabs
        let success_count = self.get_success_branches(app).len();
        let failed_count = self.get_failed_branches(app).len();

        let tab_titles = vec![
            format!("✅ Deleted ({}) ", success_count),
            format!("❌ Failed ({})", failed_count),
        ];

        let tabs = Tabs::new(tab_titles)
            .block(Block::default().borders(Borders::ALL).title("Results"))
            .select(match self.current_tab {
                ResultTab::Success => 0,
                ResultTab::Failed => 1,
            })
            .style(Style::default().fg(Color::White))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(tabs, chunks[1]);

        // Content based on current tab
        match self.current_tab {
            ResultTab::Success => {
                let branches = self.get_success_branches(app);
                let items: Vec<ListItem> = branches
                    .iter()
                    .map(|branch| {
                        let content = format!(
                            "✅ {} (target: {}, version: {})",
                            branch.name, branch.target, branch.version
                        );
                        ListItem::new(content).style(Style::default().fg(Color::Green))
                    })
                    .collect();

                if items.is_empty() {
                    let empty = Paragraph::new("No branches were successfully deleted.")
                        .style(Style::default().fg(Color::DarkGray))
                        .alignment(Alignment::Center)
                        .block(Block::default().borders(Borders::ALL).title("Deleted"));
                    f.render_widget(empty, chunks[2]);
                } else {
                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title("Deleted"))
                        .highlight_style(
                            Style::default()
                                .bg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("→ ");
                    f.render_stateful_widget(list, chunks[2], &mut self.success_list_state);
                }
            }
            ResultTab::Failed => {
                let branches = self.get_failed_branches(app);
                let items: Vec<ListItem> = branches
                    .iter()
                    .map(|branch| {
                        let error = if let CleanupStatus::Failed(e) = &branch.status {
                            e.as_str()
                        } else {
                            "Unknown error"
                        };
                        let content = format!(
                            "❌ {} (target: {}, version: {}) - {}",
                            branch.name, branch.target, branch.version, error
                        );
                        ListItem::new(content).style(Style::default().fg(Color::Red))
                    })
                    .collect();

                if items.is_empty() {
                    let empty = Paragraph::new(
                        "No failures - all selected branches were successfully deleted!",
                    )
                    .style(Style::default().fg(Color::Green))
                    .alignment(Alignment::Center)
                    .block(Block::default().borders(Borders::ALL).title("Failed"));
                    f.render_widget(empty, chunks[2]);
                } else {
                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title("Failed"))
                        .highlight_style(
                            Style::default()
                                .bg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("→ ");
                    f.render_stateful_widget(list, chunks[2], &mut self.failed_list_state);
                }
            }
        }

        // Help text
        let help_lines = vec![Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(": Switch view  "),
            Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
            Span::raw(": Navigate  "),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(": Exit"),
        ])];

        let help = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title("Help"));
        f.render_widget(help, chunks[3]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Tab => {
                self.switch_tab();
                StateChange::Keep
            }
            KeyCode::Up => {
                self.previous(app);
                StateChange::Keep
            }
            KeyCode::Down => {
                self.next(app);
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }
}
