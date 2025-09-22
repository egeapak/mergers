use crate::{
    models::AppConfig,
    ui::{state::{AppState, StateChange}, App},
    ui::state::default::DataLoadingState,
    ui::state::migration::MigrationDataLoadingState,
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub struct SettingsConfirmationState {
    config: AppConfig,
}

impl SettingsConfirmationState {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    fn create_settings_display(&self) -> Vec<Line<'_>> {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("Mode: {}", if self.config.is_migration_mode() { "Migration" } else { "Merge" }),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        // Azure DevOps Settings
        lines.push(Line::from(Span::styled(
            "Azure DevOps Settings:",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));

        let shared = self.config.shared();
        lines.push(Line::from(format!("  Organization: {}", shared.organization)));
        lines.push(Line::from(format!("  Project: {}", shared.project)));
        lines.push(Line::from(format!("  Repository: {}", shared.repository)));
        lines.push(Line::from("  PAT: ****hidden****"));
        lines.push(Line::from(""));

        // Branch Settings
        lines.push(Line::from(Span::styled(
            "Branch Settings:",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!("  Dev Branch: {}", shared.dev_branch)));
        lines.push(Line::from(format!("  Target Branch: {}", shared.target_branch)));
        if let Some(ref local_repo) = shared.local_repo {
            lines.push(Line::from(format!("  Local Repo: {}", local_repo)));
        } else {
            lines.push(Line::from("  Local Repo: [None - will clone]"));
        }
        lines.push(Line::from(""));

        // Processing Settings
        lines.push(Line::from(Span::styled(
            "Processing Settings:",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!("  Parallel Limit: {}", shared.parallel_limit)));
        lines.push(Line::from(format!("  Max Concurrent Network: {}", shared.max_concurrent_network)));
        lines.push(Line::from(format!("  Max Concurrent Processing: {}", shared.max_concurrent_processing)));
        lines.push(Line::from(format!("  Tag Prefix: {}", shared.tag_prefix)));
        if let Some(ref since) = shared.since {
            lines.push(Line::from(format!("  Since: {}", since)));
        }
        lines.push(Line::from(""));

        // Mode-Specific Settings
        lines.push(Line::from(Span::styled(
            "Mode-Specific Settings:",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));

        match &self.config {
            AppConfig::Default { default, .. } => {
                lines.push(Line::from(format!("  Work Item State: {}", default.work_item_state)));
            }
            AppConfig::Migration { migration, .. } => {
                lines.push(Line::from(format!("  Terminal States: {}", migration.terminal_states)));
            }
        }
        lines.push(Line::from(""));

        // Instructions
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::Gray)),
            Span::styled("[Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" to continue or ", Style::default().fg(Color::Gray)),
            Span::styled("[q/Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(" to exit", Style::default().fg(Color::Gray)),
        ]));

        lines
    }
}

#[async_trait]
impl AppState for SettingsConfirmationState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let area = f.area();

        // Create layout with some margins for better appearance
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)])
            .split(area.inner(Margin { horizontal: 2, vertical: 1 }));

        let settings_lines = self.create_settings_display();

        let settings_paragraph = Paragraph::new(settings_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Configuration Settings")
                    .title_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                    .border_style(Style::default().fg(Color::Blue))
            )
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Left);

        f.render_widget(settings_paragraph, layout[0]);
    }

    async fn process_key(&mut self, code: KeyCode, _app: &mut App) -> StateChange {
        match code {
            KeyCode::Enter => {
                // Proceed to appropriate data loading state
                if self.config.is_migration_mode() {
                    StateChange::Change(Box::new(MigrationDataLoadingState::new(self.config.clone())))
                } else {
                    StateChange::Change(Box::new(DataLoadingState::new()))
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                StateChange::Exit
            }
            _ => StateChange::Keep,
        }
    }
}