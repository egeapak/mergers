use crate::{
    api::AzureDevOpsClient,
    git::{cleanup_migration_worktrees, force_remove_worktree, setup_repository},
    migration::MigrationAnalyzer,
    models::AppConfig,
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
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
enum LoadingStage {
    NotStarted,
    SettingUpRepository,
    RunningAnalysis,
    Complete,
}

pub struct MigrationDataLoadingState {
    loading_stage: LoadingStage,
    loaded: bool,
    status: String,
    progress: f64,
    error: Option<String>,
    analysis_task:
        Option<tokio::task::JoinHandle<Result<crate::models::MigrationAnalysis, anyhow::Error>>>,
    config: Option<AppConfig>,
}

impl MigrationDataLoadingState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            loading_stage: LoadingStage::NotStarted,
            loaded: false,
            status: "Initializing migration analysis...".to_string(),
            progress: 0.0,
            error: None,
            analysis_task: None,
            config: Some(config),
        }
    }

    fn start_analysis(&mut self, app: &App) {
        if let Some(config) = &self.config {
            self.loading_stage = LoadingStage::SettingUpRepository;
            self.status = "Setting up repository...".to_string();
            self.progress = 0.1;

            let config = config.clone();
            let client = app.client.clone();

            // Create shared status for progress updates
            let progress_status = Arc::new(Mutex::new(String::new()));
            let progress_status_clone = progress_status.clone();

            self.analysis_task = Some(tokio::spawn(async move {
                // Generate unique ID for migration run
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let migration_id = format!("migration-{}", timestamp);

                // Setup repository for analysis
                {
                    let mut status = progress_status_clone.lock().unwrap();
                    *status = "Setting up repository...".to_string();
                }

                let repo_details = client
                    .fetch_repo_details()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to fetch repository details: {}", e))?;

                // If using local repo, attempt to clean up any existing migration worktrees
                if let Some(local_repo) = &config.shared().local_repo {
                    // Clean up the old hardcoded migration worktree
                    let _ = force_remove_worktree(
                        std::path::Path::new(local_repo),
                        "migration-analysis",
                    );
                    // Clean up any timestamped migration worktrees from previous runs
                    let _ = cleanup_migration_worktrees(std::path::Path::new(local_repo));
                }

                let repo_setup = setup_repository(
                    config.shared().local_repo.as_deref(),
                    &repo_details.ssh_url,
                    &config.shared().target_branch,
                    &migration_id,
                )
                .map_err(|e| anyhow::anyhow!("Failed to setup repository: {}", e))?;

                let repo_path = match &repo_setup {
                    crate::git::RepositorySetup::Local(path) => path.as_path(),
                    crate::git::RepositorySetup::Clone(path, _) => path.as_path(),
                };

                // Parse terminal states
                let terminal_states = match &config {
                    AppConfig::Migration { migration, .. } => {
                        AzureDevOpsClient::parse_terminal_states(&migration.terminal_states)
                    }
                    _ => unreachable!("Migration mode should have migration config"),
                };

                // Create migration analyzer
                let analyzer = MigrationAnalyzer::new(client.clone(), terminal_states);

                {
                    let mut status = progress_status_clone.lock().unwrap();
                    *status = "Running migration analysis...".to_string();
                }

                // Run migration analysis with progress callback
                let analysis = analyzer
                    .analyze_prs_for_migration(
                        repo_path,
                        &config.shared().dev_branch,
                        &config.shared().target_branch,
                        |status| {
                            let mut shared_status = progress_status_clone.lock().unwrap();
                            *shared_status = status.to_string();
                        },
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("Migration analysis failed: {}", e))?;

                // Clean up migration worktree
                if let Some(local_repo) = &config.shared().local_repo {
                    let _ = force_remove_worktree(std::path::Path::new(local_repo), &migration_id);
                }

                {
                    let mut status = progress_status_clone.lock().unwrap();
                    *status = "Analysis complete!".to_string();
                }

                Ok(analysis)
            }));

            // Store the progress status reference for updates
            // Note: In a real implementation, you might want to use a more sophisticated
            // progress tracking mechanism, but this simple approach should work
        }
    }

    async fn check_analysis_progress(&mut self, app: &mut App) -> bool {
        if let Some(task) = &mut self.analysis_task {
            if task.is_finished() {
                // Take ownership of the task to get the result
                let task = self.analysis_task.take().unwrap();
                match task.await {
                    Ok(Ok(analysis)) => {
                        self.loading_stage = LoadingStage::Complete;
                        self.status = "Analysis complete!".to_string();
                        self.progress = 1.0;
                        // Store the analysis result in the app
                        app.migration_analysis = Some(analysis);
                        return true;
                    }
                    Ok(Err(e)) => {
                        self.error = Some(e.to_string());
                        self.status = "Analysis failed".to_string();
                    }
                    Err(e) => {
                        self.error = Some(format!("Task failed: {}", e));
                        self.status = "Analysis failed".to_string();
                    }
                }
            } else {
                // Update progress based on stage
                match self.loading_stage {
                    LoadingStage::SettingUpRepository => {
                        self.progress = 0.2;
                        self.status = "Setting up repository...".to_string();
                    }
                    LoadingStage::RunningAnalysis => {
                        self.progress = 0.5;
                        self.status = "Running migration analysis...".to_string();
                    }
                    _ => {}
                }
            }
        }
        false
    }
}

#[async_trait]
impl AppState for MigrationDataLoadingState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Length(3), // Progress bar
                Constraint::Length(5), // Status
                Constraint::Min(5),    // Help/spacer
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new("Migration Analysis")
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Progress bar
        let progress_bar = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(Style::default().fg(Color::Green))
            .percent((self.progress * 100.0) as u16)
            .label(format!("{:.1}%", self.progress * 100.0));
        f.render_widget(progress_bar, chunks[1]);

        // Status
        let status_color = if self.error.is_some() {
            Color::Red
        } else if matches!(self.loading_stage, LoadingStage::Complete) {
            Color::Green
        } else {
            Color::Yellow
        };

        let status_text = if let Some(error) = &self.error {
            vec![
                Line::from(vec![Span::styled(
                    "Error:",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )]),
                Line::from(error.clone()),
            ]
        } else {
            vec![Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(&self.status, Style::default().fg(status_color)),
            ])]
        };

        let status_widget = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .wrap(Wrap { trim: true });
        f.render_widget(status_widget, chunks[2]);

        // Help text
        let help_text = if self.error.is_some() {
            vec![Line::from("Press q to quit or r to retry")]
        } else if matches!(self.loading_stage, LoadingStage::Complete) {
            vec![Line::from(
                "Analysis completed! Press any key to continue...",
            )]
        } else {
            vec![
                Line::from("Press q to cancel analysis"),
                Line::from("Please wait while we analyze your pull requests..."),
            ]
        };

        let help_widget = Paragraph::new(help_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title("Help"));
        f.render_widget(help_widget, chunks[3]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        // Start analysis on first render
        if !self.loaded && code == KeyCode::Null {
            self.loaded = true;
            if self.config.is_some() {
                self.start_analysis(app);
            }
            return StateChange::Keep;
        }

        // Check analysis progress
        if self.loaded && code == KeyCode::Null {
            if self.check_analysis_progress(app).await {
                // Analysis completed, transition to results state
                return StateChange::Change(Box::new(super::MigrationResultsState::new()));
            }
        }

        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Char('r') if self.error.is_some() => {
                // Reset for retry
                self.error = None;
                self.progress = 0.0;
                self.loading_stage = LoadingStage::NotStarted;
                self.status = "Retrying...".to_string();
                self.loaded = false;
                StateChange::Keep
            }
            _ if matches!(self.loading_stage, LoadingStage::Complete) => {
                // Any key continues after completion
                StateChange::Change(Box::new(super::MigrationResultsState::new()))
            }
            _ => StateChange::Keep,
        }
    }
}
