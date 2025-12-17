use super::CleanupModeState;
use crate::{
    git::{check_patch_merged, list_patch_branches},
    models::AppConfig,
    ui::apps::CleanupApp,
    ui::state::CleanupBranchSelectionState,
    ui::state::typed::{ModeState, StateChange},
};
use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};
use std::path::Path;

type AsyncTaskHandle<T> = tokio::task::JoinHandle<Result<T>>;

pub struct CleanupDataLoadingState {
    loaded: bool,
    status: String,
    progress: f64,
    error: Option<String>,
    loading_task: Option<AsyncTaskHandle<Vec<crate::models::CleanupBranch>>>,
}

impl Default for CleanupDataLoadingState {
    fn default() -> Self {
        Self::new(AppConfig::Cleanup {
            shared: crate::models::SharedConfig {
                organization: "default".to_string().into(),
                project: "default".to_string().into(),
                repository: "default".to_string().into(),
                pat: "default".to_string().into(),
                dev_branch: "dev".to_string().into(),
                target_branch: "next".to_string().into(),
                local_repo: None,
                parallel_limit: 300.into(),
                max_concurrent_network: 100.into(),
                max_concurrent_processing: 10.into(),
                tag_prefix: "merged-".to_string().into(),
                since: None,
                skip_confirmation: false,
            },
            cleanup: crate::models::CleanupModeConfig {
                target: "next".to_string().into(),
            },
        })
    }
}

impl CleanupDataLoadingState {
    pub fn new(_config: AppConfig) -> Self {
        Self {
            loaded: false,
            status: "Initializing cleanup analysis...".to_string(),
            progress: 0.0,
            error: None,
            loading_task: None,
        }
    }

    fn start_loading(&mut self, app: &CleanupApp) {
        if self.loading_task.is_some() {
            return;
        }

        let local_repo = app.local_repo();
        if local_repo.is_none() {
            self.error = Some(
                "No local repository path configured. Use --local-repo or path argument."
                    .to_string(),
            );
            self.loaded = true;
            return;
        }

        let repo_path = local_repo.unwrap().to_string();
        let target_branch = if let crate::models::AppConfig::Cleanup { cleanup, .. } = &*app.config
        {
            cleanup.target.value().to_string()
        } else {
            app.target_branch().to_string()
        };

        self.status = "Loading patch branches...".to_string();
        self.progress = 0.1;

        let task =
            tokio::spawn(
                async move { load_and_analyze_branches(&repo_path, &target_branch).await },
            );

        self.loading_task = Some(task);
    }

    async fn check_loading_status(&mut self) -> bool {
        if let Some(task) = self.loading_task.as_mut()
            && task.is_finished()
        {
            match task.await {
                Ok(Ok(branches)) => {
                    if branches.is_empty() {
                        self.status = "No patch branches found.".to_string();
                        self.error = Some(
                            "No patch branches matching 'patch/*' pattern were found.".to_string(),
                        );
                    } else {
                        self.status = format!("Found {} patch branches.", branches.len());
                    }
                    self.progress = 1.0;
                    self.loaded = true;
                    return true;
                }
                Ok(Err(e)) => {
                    self.error = Some(format!("Failed to load branches: {}", e));
                    self.status = "Error loading branches".to_string();
                    self.loaded = true;
                    return true;
                }
                Err(e) => {
                    self.error = Some(format!("Task error: {}", e));
                    self.status = "Task failed".to_string();
                    self.loaded = true;
                    return true;
                }
            }
        }
        false
    }
}

async fn load_and_analyze_branches(
    repo_path: &str,
    target_branch: &str,
) -> Result<Vec<crate::models::CleanupBranch>> {
    let path = Path::new(repo_path);

    // List all patch branches
    let mut branches = list_patch_branches(path)?;

    // Check which branches are merged
    for branch in &mut branches {
        let is_merged = check_patch_merged(path, &branch.name, target_branch)?;
        branch.is_merged = is_merged;
    }

    Ok(branches)
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for CleanupDataLoadingState {
    type Mode = CleanupModeState;

    fn ui(&mut self, f: &mut Frame, _app: &CleanupApp) {
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
        let title = Paragraph::new("Cleanup Mode - Loading Branches")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Progress bar
        let progress_percent = (self.progress * 100.0) as u16;
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .percent(progress_percent)
            .label(format!("{}%", progress_percent));
        f.render_widget(gauge, chunks[1]);

        // Status or error message
        let status_text = if let Some(ref error) = self.error {
            vec![
                Line::from(Span::styled(
                    "Error:",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(error.as_str()),
            ]
        } else {
            vec![Line::from(self.status.as_str())]
        };

        let status = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .wrap(Wrap { trim: true });
        f.render_widget(status, chunks[2]);

        // Help text
        let help_text = if self.error.is_some() {
            "Press 'q' to exit"
        } else {
            "Loading... Press 'q' to cancel"
        };

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(help, chunks[3]);
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut CleanupApp,
    ) -> StateChange<CleanupModeState> {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Null => {
                // Poll for task completion
                if !self.loaded {
                    if self.loading_task.is_none() {
                        self.start_loading(app);
                    }

                    if self.check_loading_status().await {
                        if self.error.is_some() {
                            // Stay in this state to show the error
                            return StateChange::Keep;
                        } else if let Some(task) = self.loading_task.take()
                            && let Ok(Ok(branches)) = task.await
                        {
                            // Update app state with loaded branches
                            *app.cleanup_branches_mut() = branches;

                            // Check if we have a local_repo path set
                            let repo_path = app.local_repo().map(std::path::PathBuf::from);
                            if let Some(path) = repo_path {
                                app.set_repo_path(Some(path));
                            }

                            // Transition to branch selection
                            return StateChange::Change(CleanupModeState::BranchSelection(
                                CleanupBranchSelectionState::new(),
                            ));
                        }
                    }
                }
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "CleanupDataLoading"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::testing::*;
    use insta::assert_snapshot;

    /// # Cleanup Data Loading Initial State Test
    ///
    /// Tests the initial loading screen when cleanup mode starts.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Renders the initial data loading screen
    /// - Shows "Initializing cleanup analysis..." message
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Loading Branches" title
    /// - Should show progress bar at 0%
    /// - Should show "Initializing cleanup analysis..." status
    /// - Should display help text for quitting/canceling
    #[test]
    fn test_data_loading_initial() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config.clone());
            let state = CleanupDataLoadingState::new(config);

            harness.render_cleanup_state(&mut CleanupModeState::DataLoading(state));
            assert_snapshot!("initial", harness.backend());
        });
    }

    /// # Cleanup Data Loading Progress Test
    ///
    /// Tests the loading screen while loading branches.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Simulates loading progress at 10%
    /// - Shows "Loading patch branches..." message
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Loading Branches" title
    /// - Should show progress bar at 10%
    /// - Should show "Loading patch branches..." status
    #[test]
    fn test_data_loading_in_progress() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config.clone());
            let mut state = CleanupDataLoadingState::new(config);

            // Simulate loading in progress
            state.status = "Loading patch branches...".to_string();
            state.progress = 0.1;

            harness.render_cleanup_state(&mut CleanupModeState::DataLoading(state));
            assert_snapshot!("in_progress", harness.backend());
        });
    }

    /// # Cleanup Data Loading Error Test
    ///
    /// Tests the loading screen when an error occurs.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Simulates an error during branch loading
    /// - Shows error message in red
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Loading Branches" title
    /// - Should show "Error:" in red
    /// - Should display the error message
    /// - Should show help text to exit
    #[test]
    fn test_data_loading_error() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config.clone());
            let mut state = CleanupDataLoadingState::new(config);

            // Simulate error
            state.error = Some("Failed to load branches: git command failed".to_string());
            state.status = "Error loading branches".to_string();
            state.loaded = true;

            harness.render_cleanup_state(&mut CleanupModeState::DataLoading(state));
            assert_snapshot!("error", harness.backend());
        });
    }

    /// # Cleanup Data Loading Complete Test
    ///
    /// Tests the loading screen when branch loading is complete.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode configuration
    /// - Simulates successful branch loading
    /// - Shows completion message
    ///
    /// ## Expected Outcome
    /// - Should display "Cleanup Mode - Loading Branches" title
    /// - Should show progress bar at 100%
    /// - Should show "Found X patch branches" message
    #[test]
    fn test_data_loading_complete() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config.clone());
            let mut state = CleanupDataLoadingState::new(config);

            // Simulate completion
            state.status = "Found 5 patch branches.".to_string();
            state.progress = 1.0;
            state.loaded = true;

            harness.render_cleanup_state(&mut CleanupModeState::DataLoading(state));
            assert_snapshot!("complete", harness.backend());
        });
    }

    /// # Cleanup Data Loading Default Test
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates CleanupDataLoadingState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should initialize with expected defaults
    #[test]
    fn test_data_loading_default() {
        let state = CleanupDataLoadingState::default();
        assert!(!state.loaded);
        assert!(state.error.is_none());
        assert!(state.loading_task.is_none());
    }

    /// # Cleanup Data Loading Quit Test
    ///
    /// Tests 'q' key to exit.
    ///
    /// ## Test Scenario
    /// - Processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_data_loading_quit() {
        let config = create_test_config_cleanup();
        let mut harness = TuiTestHarness::with_config(config.clone());
        let mut state = CleanupDataLoadingState::new(config);

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.cleanup_app_mut()).await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Cleanup Data Loading Other Keys Ignored
    ///
    /// Tests that other keys are ignored.
    ///
    /// ## Test Scenario
    /// - Processes various unrecognized keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_data_loading_other_keys() {
        let config = create_test_config_cleanup();
        let mut harness = TuiTestHarness::with_config(config.clone());
        let mut state = CleanupDataLoadingState::new(config);

        for key in [KeyCode::Up, KeyCode::Down, KeyCode::Enter, KeyCode::Esc] {
            let result = ModeState::process_key(&mut state, key, harness.cleanup_app_mut()).await;
            assert!(matches!(result, StateChange::Keep));
        }
    }

    /// # Cleanup Data Loading No Branches Found
    ///
    /// Tests display when no branches found.
    ///
    /// ## Test Scenario
    /// - Simulates empty branch result
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should show "No patch branches found" error
    #[test]
    fn test_data_loading_no_branches() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config.clone());
            let mut state = CleanupDataLoadingState::new(config);

            // Simulate no branches found
            state.status = "No patch branches found.".to_string();
            state.error =
                Some("No patch branches matching 'patch/*' pattern were found.".to_string());
            state.loaded = true;

            harness.render_cleanup_state(&mut CleanupModeState::DataLoading(state));
            assert_snapshot!("no_branches", harness.backend());
        });
    }

    /// # Cleanup Data Loading Half Progress
    ///
    /// Tests progress display at 50%.
    ///
    /// ## Test Scenario
    /// - Simulates loading at 50%
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should show progress bar at 50%
    #[test]
    fn test_data_loading_half_progress() {
        use crate::ui::snapshot_testing::with_settings_and_module_path;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_cleanup();
            let mut harness = TuiTestHarness::with_config(config.clone());
            let mut state = CleanupDataLoadingState::new(config);

            state.status = "Analyzing branch merge status...".to_string();
            state.progress = 0.5;

            harness.render_cleanup_state(&mut CleanupModeState::DataLoading(state));
            assert_snapshot!("half_progress", harness.backend());
        });
    }
}
