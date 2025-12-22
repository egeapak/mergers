//! Typed settings confirmation state that works with any app mode.
//!
//! This module provides [`TypedSettingsConfirmationState`], a generic settings
//! confirmation state that can work with any mode-specific app type.

use crate::models::AppConfig;
use crate::parsed_property::ParsedProperty;
use crate::ui::AppMode;
use crate::ui::state::typed::StateChange;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::marker::PhantomData;

/// Typed settings confirmation state - works with any app mode.
///
/// This state displays the application configuration and waits for
/// user confirmation before proceeding. It is generic over the app type,
/// allowing it to be used with any mode (Merge, Migration, Cleanup).
///
/// # Type Parameters
///
/// * `A` - The app mode type (must implement [`AppMode`])
/// * `S` - The state enum type for this mode
///
/// # Example
///
/// ```ignore
/// use crate::ui::apps::MergeApp;
/// use crate::ui::state::default::MergeState;
///
/// // Create typed settings confirmation for merge mode
/// let state: TypedSettingsConfirmationState<MergeApp, MergeState> =
///     TypedSettingsConfirmationState::new(config);
/// ```
pub struct TypedSettingsConfirmationState<A, S> {
    config: AppConfig,
    _app: PhantomData<A>,
    _state: PhantomData<S>,
}

impl<A, S> std::fmt::Debug for TypedSettingsConfirmationState<A, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedSettingsConfirmationState")
            .field("config", &self.config)
            .finish()
    }
}

impl<A, S> TypedSettingsConfirmationState<A, S> {
    /// Create a new typed settings confirmation state.
    ///
    /// # Arguments
    ///
    /// * `config` - The application configuration to display
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            _app: PhantomData,
            _state: PhantomData,
        }
    }

    /// Get a reference to the stored configuration.
    pub fn config(&self) -> &AppConfig {
        &self.config
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
        let mode_name = match &self.config {
            AppConfig::Default { .. } => "Merge",
            AppConfig::Migration { .. } => "Migration",
            AppConfig::Cleanup { .. } => "Cleanup",
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("Mode: {}", mode_name),
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

        // Special handling for since field
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
            AppConfig::Cleanup { cleanup, .. } => {
                lines.push(self.format_property_with_source("Target Branch", &cleanup.target));
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

impl<A, S> TypedSettingsConfirmationState<A, S>
where
    A: AppMode + Send + Sync,
    S: Send + Sync + 'static,
{
    /// Render the settings confirmation UI.
    pub fn render(&self, f: &mut Frame) {
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

    /// Handle key input.
    pub fn handle_key<R, F>(&self, code: KeyCode, make_next_state: F) -> StateChange<R>
    where
        F: FnOnce(&AppConfig) -> R,
    {
        match code {
            KeyCode::Enter => {
                // Create the next state using the provided function
                StateChange::Change(make_next_state(&self.config))
            }
            KeyCode::Char('q') | KeyCode::Esc => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }

    /// Get this state's name for logging/debugging.
    pub fn name(&self) -> &'static str {
        "SettingsConfirmation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DefaultModeConfig, SharedConfig};

    fn create_test_config() -> AppConfig {
        AppConfig::Default {
            shared: SharedConfig {
                organization: ParsedProperty::Default("test-org".to_string()),
                project: ParsedProperty::Default("test-project".to_string()),
                repository: ParsedProperty::Default("test-repo".to_string()),
                pat: ParsedProperty::Default("test-pat".to_string()),
                dev_branch: ParsedProperty::Default("develop".to_string()),
                target_branch: ParsedProperty::Default("main".to_string()),
                local_repo: None,
                parallel_limit: ParsedProperty::Default(4),
                max_concurrent_network: ParsedProperty::Default(10),
                max_concurrent_processing: ParsedProperty::Default(5),
                tag_prefix: ParsedProperty::Default("merged/".to_string()),
                since: None,
                skip_confirmation: false,
            },
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
                run_hooks: ParsedProperty::Default(false),
            },
        }
    }

    /// # TypedSettingsConfirmationState New Constructor
    ///
    /// Tests that TypedSettingsConfirmationState::new() works correctly.
    ///
    /// ## Test Scenario
    /// - Creates a TypedSettingsConfirmationState using new()
    ///
    /// ## Expected Outcome
    /// - Should create successfully and have correct name
    #[test]
    fn test_typed_settings_confirmation_new() {
        let config = create_test_config();
        let state: TypedSettingsConfirmationState<
            crate::ui::apps::MergeApp,
            crate::ui::state::default::MergeState,
        > = TypedSettingsConfirmationState::new(config);
        assert_eq!(state.name(), "SettingsConfirmation");
    }

    /// # TypedSettingsConfirmationState Config Access
    ///
    /// Tests that config() returns the stored configuration.
    ///
    /// ## Test Scenario
    /// - Creates a TypedSettingsConfirmationState with a config
    /// - Accesses config via the getter
    ///
    /// ## Expected Outcome
    /// - Should return reference to stored config
    #[test]
    fn test_typed_settings_confirmation_config_access() {
        let config = create_test_config();
        let state: TypedSettingsConfirmationState<
            crate::ui::apps::MergeApp,
            crate::ui::state::default::MergeState,
        > = TypedSettingsConfirmationState::new(config);

        assert!(matches!(state.config(), AppConfig::Default { .. }));
    }

    /// # TypedSettingsConfirmationState Debug Implementation
    ///
    /// Tests that TypedSettingsConfirmationState implements Debug correctly.
    ///
    /// ## Test Scenario
    /// - Creates a TypedSettingsConfirmationState
    /// - Formats it using Debug
    ///
    /// ## Expected Outcome
    /// - Should produce readable debug output
    #[test]
    fn test_typed_settings_confirmation_debug() {
        let config = create_test_config();
        let state: TypedSettingsConfirmationState<
            crate::ui::apps::MergeApp,
            crate::ui::state::default::MergeState,
        > = TypedSettingsConfirmationState::new(config);
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("TypedSettingsConfirmationState"));
    }
}
