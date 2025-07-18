use crate::{
    api::AzureDevOpsClient,
    git::{cleanup_migration_worktrees, force_remove_worktree, setup_repository},
    migration::MigrationAnalyzer,
    models::{AppConfig, PullRequest, PullRequestWithWorkItems, WorkItem},
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
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
enum LoadingStage {
    NotStarted,
    FetchingPullRequests,
    SettingUpRepository,
    FetchingWorkItems,
    WaitingForWorkItems,
    RunningAnalysis,
    Complete,
}

pub struct MigrationDataLoadingState {
    loading_stage: LoadingStage,
    loaded: bool,
    status: String,
    progress: f64,
    error: Option<String>,
    config: Option<AppConfig>,

    // Task management
    pr_fetch_task: Option<tokio::task::JoinHandle<Result<Vec<PullRequest>, String>>>,
    repo_setup_task:
        Option<tokio::task::JoinHandle<Result<(std::path::PathBuf, Vec<String>), String>>>,
    work_items_tasks: Option<Vec<tokio::task::JoinHandle<Result<(usize, Vec<WorkItem>), String>>>>,
    analysis_task:
        Option<tokio::task::JoinHandle<Result<crate::models::MigrationAnalysis, String>>>,

    // Progress tracking
    total_prs: usize,
    work_items_fetched: usize,
    work_items_total: usize,
    migration_id: String,

    // Intermediate results
    prs: Vec<PullRequest>,
    prs_with_work_items: Vec<PullRequestWithWorkItems>,
    repo_path: Option<std::path::PathBuf>,
    terminal_states: Option<Vec<String>>,
}

impl MigrationDataLoadingState {
    pub fn new(config: AppConfig) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            loading_stage: LoadingStage::NotStarted,
            loaded: false,
            status: "Initializing migration analysis...".to_string(),
            progress: 0.0,
            error: None,
            config: Some(config),
            pr_fetch_task: None,
            repo_setup_task: None,
            work_items_tasks: None,
            analysis_task: None,
            total_prs: 0,
            work_items_fetched: 0,
            work_items_total: 0,
            migration_id: format!("migration-{}", timestamp),
            prs: Vec::new(),
            prs_with_work_items: Vec::new(),
            repo_path: None,
            terminal_states: None,
        }
    }

    async fn start_pr_fetching(&mut self, app: &App) -> Result<(), String> {
        self.loading_stage = LoadingStage::FetchingPullRequests;
        self.status = "Fetching pull requests...".to_string();
        self.progress = 0.1;

        let client = app.client.clone();
        let dev_branch = app.dev_branch.clone();

        self.pr_fetch_task = Some(tokio::spawn(async move {
            let prs = client
                .fetch_pull_requests(&dev_branch)
                .await
                .map_err(|e| format!("Failed to fetch pull requests: {}", e))?;

            // For migration mode, we want all PRs, not just untagged ones
            Ok(prs)
        }));

        Ok(())
    }

    async fn check_pr_fetch_progress(&mut self) -> Result<Option<Vec<PullRequest>>, String> {
        if let Some(task) = &mut self.pr_fetch_task {
            if task.is_finished() {
                let task = self.pr_fetch_task.take().unwrap();
                match task.await {
                    Ok(Ok(prs)) => {
                        self.total_prs = prs.len();
                        return Ok(Some(prs));
                    }
                    Ok(Err(e)) => {
                        return Err(e);
                    }
                    Err(e) => {
                        return Err(format!("PR fetch task failed: {}", e));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn start_repository_setup(&mut self) -> Result<(), String> {
        if let Some(config) = &self.config {
            self.loading_stage = LoadingStage::SettingUpRepository;
            self.status = "Setting up repository...".to_string();
            self.progress = 0.2;

            let config = config.clone();
            let migration_id = self.migration_id.clone();

            self.repo_setup_task = Some(tokio::spawn(async move {
                Self::perform_repository_setup(config, migration_id).await
            }));
        }
        Ok(())
    }

    async fn perform_repository_setup(
        config: AppConfig,
        migration_id: String,
    ) -> Result<(std::path::PathBuf, Vec<String>), String> {
        // Create client from config
        let client = AzureDevOpsClient::new(
            config.shared().organization.clone(),
            config.shared().project.clone(),
            config.shared().repository.clone(),
            config.shared().pat.clone(),
        )
        .map_err(|e| format!("Failed to create client: {}", e))?;

        // Setup repository for analysis
        let repo_details = client
            .fetch_repo_details()
            .await
            .map_err(|e| format!("Failed to fetch repository details: {}", e))?;

        // If using local repo, attempt to clean up any existing migration worktrees
        if let Some(local_repo) = &config.shared().local_repo {
            // Clean up the old hardcoded migration worktree
            let _ = force_remove_worktree(std::path::Path::new(local_repo), "migration-analysis");
            // Clean up any timestamped migration worktrees from previous runs
            let _ = cleanup_migration_worktrees(std::path::Path::new(local_repo));
        }

        let repo_setup = setup_repository(
            config.shared().local_repo.as_deref(),
            &repo_details.ssh_url,
            &config.shared().target_branch,
            &migration_id,
        )
        .map_err(|e| format!("Failed to setup repository: {}", e))?;

        let repo_path = match &repo_setup {
            crate::git::RepositorySetup::Local(path) => path.to_path_buf(),
            crate::git::RepositorySetup::Clone(path, _) => path.to_path_buf(),
        };

        // Parse terminal states
        let terminal_states = match &config {
            AppConfig::Migration { migration, .. } => {
                AzureDevOpsClient::parse_terminal_states(&migration.terminal_states)
            }
            _ => return Err("Migration mode should have migration config".to_string()),
        };

        Ok((repo_path, terminal_states))
    }

    async fn check_repository_setup_progress(&mut self) -> Result<bool, String> {
        if let Some(task) = &mut self.repo_setup_task {
            if task.is_finished() {
                let task = self.repo_setup_task.take().unwrap();
                match task.await {
                    Ok(Ok((repo_path, terminal_states))) => {
                        self.repo_path = Some(repo_path);
                        self.terminal_states = Some(terminal_states);
                        return Ok(true);
                    }
                    Ok(Err(e)) => {
                        return Err(e);
                    }
                    Err(e) => {
                        return Err(format!("Repository setup task failed: {}", e));
                    }
                }
            }
        }
        Ok(false)
    }

    fn start_work_items_fetching(&mut self, app: &App) {
        self.loading_stage = LoadingStage::FetchingWorkItems;
        self.work_items_total = self.prs.len();
        self.work_items_fetched = 0;
        self.status = "Fetching work items for PRs...".to_string();
        self.progress = 0.3;

        // Start parallel tasks for fetching work items
        let mut tasks = Vec::new();

        for (index, pr) in self.prs.iter().enumerate() {
            let client = app.client.clone();
            let pr_id = pr.id;

            let task = tokio::spawn(async move {
                let work_items = client
                    .fetch_work_items_with_history_for_pr(pr_id)
                    .await
                    .unwrap_or_default();
                Ok((index, work_items))
            });

            tasks.push(task);
        }

        self.work_items_tasks = Some(tasks);
    }

    async fn check_work_items_progress(&mut self) -> Result<bool, String> {
        if let Some(ref mut tasks) = self.work_items_tasks {
            let mut completed = Vec::new();
            let mut still_running = Vec::new();

            // Check which tasks have completed
            for task in tasks.drain(..) {
                if task.is_finished() {
                    match task.await {
                        Ok(Ok((index, work_items))) => {
                            completed.push((index, work_items));
                        }
                        Ok(Err(e)) => {
                            return Err(format!("Failed to fetch work items: {}", e));
                        }
                        Err(e) => {
                            return Err(format!("Work items task failed: {}", e));
                        }
                    }
                } else {
                    still_running.push(task);
                }
            }

            // Update completed work items
            for (index, work_items) in completed {
                if let Some(pr) = self.prs.get(index) {
                    self.prs_with_work_items.push(PullRequestWithWorkItems {
                        pr: pr.clone(),
                        work_items,
                        selected: false,
                    });
                    self.work_items_fetched += 1;
                }
            }

            *tasks = still_running;

            // Update progress
            if self.work_items_total > 0 {
                self.progress =
                    0.3 + (0.3 * self.work_items_fetched as f64 / self.work_items_total as f64);
                self.status = format!(
                    "Fetching work items ({}/{})",
                    self.work_items_fetched, self.work_items_total
                );
            }

            // Return true if all tasks are completed
            if tasks.is_empty() {
                self.work_items_tasks = None;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(true) // No tasks means we're done
        }
    }

    async fn start_migration_analysis(&mut self) -> Result<(), String> {
        if let (Some(repo_path), Some(terminal_states), Some(config)) =
            (&self.repo_path, &self.terminal_states, &self.config)
        {
            self.loading_stage = LoadingStage::RunningAnalysis;
            self.status = "Running migration analysis...".to_string();
            self.progress = 0.7;

            let prs_with_work_items = self.prs_with_work_items.clone();
            let repo_path = repo_path.clone();
            let terminal_states = terminal_states.clone();
            let config = config.clone();
            let migration_id = self.migration_id.clone();

            self.analysis_task = Some(tokio::spawn(async move {
                Self::perform_migration_analysis(
                    prs_with_work_items,
                    repo_path,
                    terminal_states,
                    config,
                    migration_id,
                )
                .await
            }));
        }
        Ok(())
    }

    async fn perform_migration_analysis(
        prs_with_work_items: Vec<PullRequestWithWorkItems>,
        repo_path: std::path::PathBuf,
        terminal_states: Vec<String>,
        config: AppConfig,
        migration_id: String,
    ) -> Result<crate::models::MigrationAnalysis, String> {
        // Create client from config
        let client = AzureDevOpsClient::new(
            config.shared().organization.clone(),
            config.shared().project.clone(),
            config.shared().repository.clone(),
            config.shared().pat.clone(),
        )
        .map_err(|e| format!("Failed to create client: {}", e))?;

        // Create migration analyzer
        let analyzer = MigrationAnalyzer::new(client, terminal_states);

        // Calculate git symmetric difference
        let symmetric_diff = crate::git::get_symmetric_difference(
            &repo_path,
            &config.shared().dev_branch,
            &config.shared().target_branch,
        )
        .map_err(|e| format!("Failed to calculate git diff: {}", e))?;

        // Analyze each PR in parallel
        let mut analysis_tasks = Vec::new();

        for pr_with_work_items in prs_with_work_items.iter() {
            let analyzer_clone = analyzer.clone();
            let pr_clone = pr_with_work_items.clone();
            let symmetric_diff_clone = symmetric_diff.clone();
            let repo_path_clone = repo_path.clone();
            let target_branch = config.shared().target_branch.clone();

            let task = tokio::spawn(async move {
                analyzer_clone
                    .analyze_single_pr(
                        &pr_clone,
                        &symmetric_diff_clone,
                        &repo_path_clone,
                        &target_branch,
                    )
                    .await
            });

            analysis_tasks.push(task);
        }

        // Wait for all analyses to complete
        let mut pr_analyses = Vec::new();
        for task in analysis_tasks {
            match task.await {
                Ok(Ok(analysis)) => pr_analyses.push(analysis),
                Ok(Err(e)) => return Err(format!("PR analysis failed: {}", e)),
                Err(e) => return Err(format!("PR analysis task failed: {}", e)),
            }
        }

        // Categorize PRs
        let analysis = analyzer
            .categorize_prs(pr_analyses, symmetric_diff)
            .map_err(|e| format!("Failed to categorize PRs: {}", e))?;

        // Clean up migration worktree
        if let Some(local_repo) = &config.shared().local_repo {
            let _ = force_remove_worktree(std::path::Path::new(local_repo), &migration_id);
        }

        Ok(analysis)
    }

    async fn check_analysis_progress(&mut self, app: &mut App) -> Result<bool, String> {
        if let Some(task) = &mut self.analysis_task {
            if task.is_finished() {
                let task = self.analysis_task.take().unwrap();
                match task.await {
                    Ok(Ok(analysis)) => {
                        self.loading_stage = LoadingStage::Complete;
                        self.status = "Analysis complete!".to_string();
                        self.progress = 1.0;
                        app.migration_analysis = Some(analysis);
                        return Ok(true);
                    }
                    Ok(Err(e)) => {
                        return Err(e);
                    }
                    Err(e) => {
                        return Err(format!("Analysis task failed: {}", e));
                    }
                }
            } else {
                // Update progress while analysis is running
                if self.progress < 0.95 {
                    self.progress += 0.01; // Slowly increment progress
                }
                self.status = format!("Analyzing {} PRs...", self.total_prs);
            }
        }
        Ok(false)
    }

    fn get_loading_message(&self) -> String {
        match self.loading_stage {
            LoadingStage::NotStarted => "Initializing...".to_string(),
            LoadingStage::FetchingPullRequests => "Fetching pull requests...".to_string(),
            LoadingStage::SettingUpRepository => "Setting up repository...".to_string(),
            LoadingStage::FetchingWorkItems => {
                if self.work_items_total > 0 {
                    format!(
                        "Fetching work items ({}/{})",
                        self.work_items_fetched, self.work_items_total
                    )
                } else {
                    "Fetching work items...".to_string()
                }
            }
            LoadingStage::WaitingForWorkItems => {
                format!(
                    "Fetching work items ({}/{})",
                    self.work_items_fetched, self.work_items_total
                )
            }
            LoadingStage::RunningAnalysis => {
                format!("Analyzing {} PRs...", self.total_prs)
            }
            LoadingStage::Complete => "Analysis complete".to_string(),
        }
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
            let loading_message = self.get_loading_message();
            vec![Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(loading_message, Style::default().fg(status_color)),
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
        // Start loading on first render
        if !self.loaded && code == KeyCode::Null {
            self.loaded = true;
            if let Err(e) = self.start_pr_fetching(app).await {
                self.error = Some(e);
                return StateChange::Keep;
            }
            return StateChange::Keep;
        }

        // Process loading stages
        if self.loaded && code == KeyCode::Null {
            match self.loading_stage {
                LoadingStage::FetchingPullRequests => {
                    match self.check_pr_fetch_progress().await {
                        Ok(Some(prs)) => {
                            self.prs = prs;
                            if let Err(e) = self.start_repository_setup().await {
                                self.error = Some(e);
                            }
                        }
                        Ok(None) => {
                            // Still fetching, continue
                        }
                        Err(e) => {
                            self.error = Some(e);
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::SettingUpRepository => {
                    match self.check_repository_setup_progress().await {
                        Ok(true) => {
                            // Repository setup complete, start fetching work items
                            self.start_work_items_fetching(app);
                        }
                        Ok(false) => {
                            // Still setting up, continue
                        }
                        Err(e) => {
                            self.error = Some(e);
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingWorkItems => {
                    self.loading_stage = LoadingStage::WaitingForWorkItems;
                    return StateChange::Keep;
                }
                LoadingStage::WaitingForWorkItems => {
                    match self.check_work_items_progress().await {
                        Ok(true) => {
                            // Work items complete, start migration analysis
                            if let Err(e) = self.start_migration_analysis().await {
                                self.error = Some(e);
                            }
                        }
                        Ok(false) => {
                            // Still fetching work items, continue
                        }
                        Err(e) => {
                            self.error = Some(e);
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::RunningAnalysis => {
                    match self.check_analysis_progress(app).await {
                        Ok(true) => {
                            // Analysis completed, transition to results state
                            return StateChange::Change(Box::new(
                                super::MigrationResultsState::new(),
                            ));
                        }
                        Ok(false) => {
                            // Still analyzing, continue
                        }
                        Err(e) => {
                            self.error = Some(e);
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::Complete => {
                    // Should transition to results, but handle just in case
                    return StateChange::Change(Box::new(super::MigrationResultsState::new()));
                }
                LoadingStage::NotStarted => {
                    // Should not happen, but handle gracefully
                    return StateChange::Keep;
                }
            }
        }

        // Handle user input
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            KeyCode::Char('r') if self.error.is_some() => {
                // Reset for retry
                self.error = None;
                self.progress = 0.0;
                self.loading_stage = LoadingStage::NotStarted;
                self.status = "Retrying...".to_string();
                self.loaded = false;
                self.pr_fetch_task = None;
                self.repo_setup_task = None;
                self.work_items_tasks = None;
                self.analysis_task = None;
                self.total_prs = 0;
                self.work_items_fetched = 0;
                self.work_items_total = 0;
                self.prs.clear();
                self.prs_with_work_items.clear();
                self.repo_path = None;
                self.terminal_states = None;
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
