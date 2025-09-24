use crate::{
    models::AppConfig,
    parsed_property::ParsedProperty,
    ui::state::default::DataLoadingState,
    ui::state::migration::MigrationDataLoadingState,
    ui::{
        App,
        state::{AppState, StateChange},
    },
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

    fn format_property_with_source<T: std::fmt::Display>(
        &self,
        label: &str,
        property: &ParsedProperty<T>,
    ) -> Line<'_> {
        match property {
            ParsedProperty::Git(value, url) => Line::from(vec![
                Span::styled(format!("{}: ", label), Style::default()),
                Span::styled(
                    value.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    format!(" [from git: {}]", url),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
            ParsedProperty::Env(value, env_var) => Line::from(vec![
                Span::styled(format!("{}: ", label), Style::default()),
                Span::styled(
                    value.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    format!(" [from env: {}]", env_var.split('=').next().unwrap_or("")),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
            ParsedProperty::File(value, path, _) => Line::from(vec![
                Span::styled(format!("{}: ", label), Style::default()),
                Span::styled(
                    value.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    " [from config file: ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    format!("{:?}", path),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "]",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
            ParsedProperty::Cli(value, _) => Line::from(vec![
                Span::styled(format!("{}: ", label), Style::default()),
                Span::styled(
                    value.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    " [from cli]",
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
            ParsedProperty::Default(value) => Line::from(vec![
                Span::styled(format!("{}: ", label), Style::default()),
                Span::styled(
                    value.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    " [default]",
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
        }
    }

    fn create_settings_display(&self) -> Vec<Line<'_>> {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!(
                    "Mode: {}",
                    if self.config.is_migration_mode() {
                        "Migration"
                    } else {
                        "Merge"
                    }
                ),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        let shared = self.config.shared();

        // Azure DevOps Settings
        lines.push(Line::from(Span::styled(
            "Azure DevOps Settings:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));

        lines.push(self.format_property_with_source("Organization", &shared.organization));
        lines.push(self.format_property_with_source("Project", &shared.project));
        lines.push(self.format_property_with_source("Repository", &shared.repository));
        lines.push(Line::from("  PAT: ****hidden****"));
        lines.push(Line::from(""));

        // Branch Settings
        lines.push(Line::from(Span::styled(
            "Branch Settings:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.format_property_with_source("Dev Branch", &shared.dev_branch));
        lines.push(self.format_property_with_source("Target Branch", &shared.target_branch));
        if let Some(ref local_repo) = shared.local_repo {
            lines.push(self.format_property_with_source("Local Repo", local_repo));
        } else {
            lines.push(Line::from("  Local Repo: [None - will clone]"));
        }
        lines.push(Line::from(""));

        // Processing Settings
        lines.push(Line::from(Span::styled(
            "Processing Settings:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.format_property_with_source("Parallel Limit", &shared.parallel_limit));
        lines.push(
            self.format_property_with_source(
                "Max Concurrent Network",
                &shared.max_concurrent_network,
            ),
        );
        lines.push(self.format_property_with_source(
            "Max Concurrent Processing",
            &shared.max_concurrent_processing,
        ));
        lines.push(self.format_property_with_source("Tag Prefix", &shared.tag_prefix));

        // Special handling for since field showing both original and parsed value
        if let Some(ref since) = shared.since {
            let formatted_date = since.value().format("%Y-%m-%d %H:%M:%S UTC");
            match since {
                ParsedProperty::Cli(_, original) => {
                    lines.push(Line::from(vec![
                        Span::styled("  Since: ", Style::default()),
                        Span::styled(original, Style::default()),
                        Span::styled(" (resolves to: ", Style::default().fg(Color::Gray)),
                        Span::styled(
                            formatted_date.to_string(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::ITALIC),
                        ),
                        Span::styled(")", Style::default().fg(Color::Gray)),
                    ]));
                }
                _ => {
                    lines.push(Line::from(vec![
                        Span::styled("  Since: ", Style::default()),
                        Span::styled(formatted_date.to_string(), Style::default().fg(Color::Cyan)),
                    ]));
                }
            }
        }
        lines.push(Line::from(""));

        // Mode-Specific Settings
        lines.push(Line::from(Span::styled(
            "Mode-Specific Settings:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));

        match &self.config {
            AppConfig::Default { default, .. } => {
                lines.push(
                    self.format_property_with_source("Work Item State", &default.work_item_state),
                );
            }
            AppConfig::Migration { migration, .. } => {
                // Special handling for terminal states showing both original and parsed
                let states = format!("[{}]", migration.terminal_states.value().join(", "));
                lines.push(Line::from(vec![
                    Span::styled("  Terminal States: ", Style::default()),
                    Span::styled(
                        states,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
        }
        lines.push(Line::from(""));

        // Instructions
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::Gray)),
            Span::styled(
                "[Enter]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to continue or ", Style::default().fg(Color::Gray)),
            Span::styled(
                "[q/Esc]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
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
            .split(area.inner(Margin {
                horizontal: 2,
                vertical: 1,
            }));

        let settings_lines = self.create_settings_display();

        let settings_paragraph = Paragraph::new(settings_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Configuration Settings")
                    .title_style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )
                    .border_style(Style::default().fg(Color::Blue)),
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
                    StateChange::Change(Box::new(MigrationDataLoadingState::new(
                        self.config.clone(),
                    )))
                } else {
                    StateChange::Change(Box::new(DataLoadingState::new()))
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }
}
