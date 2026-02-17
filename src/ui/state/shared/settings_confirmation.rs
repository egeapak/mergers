use crate::{models::AppConfig, parsed_property::ParsedProperty, ui::state::typed::StateChange};
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

    /// Get a reference to the config.
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Render the settings confirmation UI.
    ///
    /// This is a mode-agnostic rendering method that can be called from
    /// any mode's AppState implementation.
    pub fn render(&mut self, f: &mut Frame) {
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

    /// Handle a key press and return the typed state change.
    ///
    /// This is a mode-agnostic key handler that takes a closure to construct
    /// the next state when Enter is pressed. Each mode can provide its own
    /// data loading state constructor.
    ///
    /// # Arguments
    ///
    /// * `code` - The key code pressed
    /// * `make_next_state` - A closure that takes the config and returns the next state
    pub fn handle_key<S, F>(&self, code: KeyCode, make_next_state: F) -> StateChange<S>
    where
        F: FnOnce(&AppConfig) -> S,
    {
        match code {
            KeyCode::Enter => StateChange::Change(make_next_state(&self.config)),
            KeyCode::Char('q') | KeyCode::Esc => StateChange::Exit,
            _ => StateChange::Keep,
        }
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
            AppConfig::ReleaseNotes { .. } => "Release Notes",
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
            AppConfig::Cleanup { cleanup, .. } => {
                lines.push(self.format_property_with_source("Target Branch", &cleanup.target));
            }
            AppConfig::ReleaseNotes { .. } => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::testing::*;
    use insta::assert_snapshot;

    /// # Settings Confirmation Default Mode Test
    ///
    /// Tests the settings confirmation screen for default (merge) mode with mixed configuration sources.
    ///
    /// ## Test Scenario
    /// - Creates a default mode configuration with values from different sources (CLI, env, file, git, default)
    /// - Renders the settings confirmation screen in a fixed 80x30 terminal
    /// - Captures the complete UI output for snapshot comparison
    ///
    /// ## Expected Outcome
    /// - Should display "Mode: Merge" at the top
    /// - Should show Azure DevOps settings with source annotations
    /// - Should show branch settings with appropriate source colors/styling
    /// - Should show processing settings with default values
    /// - Should display work item state for default mode
    /// - Should show navigation instructions at the bottom
    #[test]
    fn test_settings_confirmation_default_mode() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let config_for_state = config.clone();
            let mut harness = TuiTestHarness::with_config(config);
            let mut state = SettingsConfirmationState::new(config_for_state);

            harness.terminal.draw(|f| state.render(f)).unwrap();
            assert_snapshot!("default_mode", harness.backend());
        });
    }

    /// # Settings Confirmation Migration Mode Test
    ///
    /// Tests the settings confirmation screen for migration mode with terminal states.
    ///
    /// ## Test Scenario
    /// - Creates a migration mode configuration
    /// - Renders the settings confirmation screen
    /// - Captures the UI output showing migration-specific settings
    ///
    /// ## Expected Outcome
    /// - Should display "Mode: Migration" at the top
    /// - Should show terminal states instead of work item state
    /// - Should display all other settings sections normally
    #[test]
    fn test_settings_confirmation_migration_mode() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let config_for_state = config.clone();
            let mut harness = TuiTestHarness::with_config(config);
            let mut state = SettingsConfirmationState::new(config_for_state);

            harness.terminal.draw(|f| state.render(f)).unwrap();
            assert_snapshot!("migration_mode", harness.backend());
        });
    }

    /// # Settings Confirmation Cleanup Mode Test
    ///
    /// Tests the settings confirmation screen for cleanup mode.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Renders the settings confirmation screen
    /// - Captures the UI output showing cleanup-specific settings
    ///
    /// ## Expected Outcome
    /// - Should display "Mode: Cleanup" at the top
    /// - Should show target branch in mode-specific settings
    /// - Should display all other settings sections normally
    #[test]
    fn test_settings_confirmation_cleanup_mode() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let config_for_state = config.clone();
            let mut harness = TuiTestHarness::with_config(config);
            let mut state = SettingsConfirmationState::new(config_for_state);

            harness.terminal.draw(|f| state.render(f)).unwrap();
            assert_snapshot!("cleanup_mode", harness.backend());
        });
    }

    /// # Settings Confirmation All Defaults Test
    ///
    /// Tests the settings confirmation screen with all default values.
    ///
    /// ## Test Scenario
    /// - Creates a configuration where all values are defaults
    /// - No local repo path configured
    /// - No since date configured
    /// - Renders the complete settings display
    ///
    /// ## Expected Outcome
    /// - All settings should show "[default]" source annotation
    /// - Local repo should show "[None - will clone]"
    /// - Since date should not appear in the display
    /// - All values should be rendered with default styling
    #[test]
    fn test_settings_confirmation_all_defaults() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_all_defaults();
            let config_for_state = config.clone();
            let mut harness = TuiTestHarness::with_config(config);
            let mut state = SettingsConfirmationState::new(config_for_state);

            harness.terminal.draw(|f| state.render(f)).unwrap();
            assert_snapshot!("all_defaults", harness.backend());
        });
    }

    /// # Settings Confirmation CLI Values Test
    ///
    /// Tests the settings confirmation screen with values provided via CLI arguments.
    ///
    /// ## Test Scenario
    /// - Creates a configuration where most values come from CLI
    /// - Includes a since date with original CLI input and parsed value
    /// - Local repo path provided via CLI
    /// - Custom parallel processing limits
    ///
    /// ## Expected Outcome
    /// - All CLI values should show "[from cli]" annotation with light blue styling
    /// - Since date should show both original input and resolved datetime
    /// - Local repo should display the CLI-provided path
    /// - Custom work item state should be displayed
    #[test]
    fn test_settings_confirmation_cli_values() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cli_values();
            let config_for_state = config.clone();
            let mut harness = TuiTestHarness::with_config(config);
            let mut state = SettingsConfirmationState::new(config_for_state);

            harness.terminal.draw(|f| state.render(f)).unwrap();
            assert_snapshot!("cli_values", harness.backend());
        });
    }

    /// # Settings Confirmation Environment Variables Test
    ///
    /// Tests the settings confirmation screen with values from environment variables.
    ///
    /// ## Test Scenario
    /// - Creates a configuration with environment variable sources
    /// - Mix of env vars and default values
    /// - No local repo or since date configured
    ///
    /// ## Expected Outcome
    /// - Environment values should show "[from env: VAR_NAME]" annotation with blue styling
    /// - Should display the environment variable names that provided the values
    /// - Default values should still show "[default]" annotation
    /// - Local repo should show "[None - will clone]"
    #[test]
    fn test_settings_confirmation_env_values() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_env_values();
            let config_for_state = config.clone();
            let mut harness = TuiTestHarness::with_config(config);
            let mut state = SettingsConfirmationState::new(config_for_state);

            harness.terminal.draw(|f| state.render(f)).unwrap();
            assert_snapshot!("env_values", harness.backend());
        });
    }

    /// # Settings Confirmation File Values Test
    ///
    /// Tests the settings confirmation screen with values from configuration file.
    ///
    /// ## Test Scenario
    /// - Creates a configuration with file-based values
    /// - Shows config file path in source annotations
    /// - Mix of file values and defaults
    /// - Local repo path from config file
    ///
    /// ## Expected Outcome
    /// - File values should show "[from config file: /path/to/config.toml]" with magenta styling
    /// - Should display the full config file path
    /// - Local repo should show the file-provided path
    /// - PAT should still show as "****hidden****"
    #[test]
    fn test_settings_confirmation_file_values() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_file_values();
            let config_for_state = config.clone();
            let mut harness = TuiTestHarness::with_config(config);
            let mut state = SettingsConfirmationState::new(config_for_state);

            harness.terminal.draw(|f| state.render(f)).unwrap();
            assert_snapshot!("file_values", harness.backend());
        });
    }
}
