use crate::{
    api::AzureDevOpsClient,
    git::{
        cleanup_migration_worktrees, force_remove_worktree, get_target_branch_history,
        setup_repository,
    },
    migration::MigrationAnalyzer,
    models::{AppConfig, PullRequest, PullRequestWithWorkItems, WorkItem},
    ui::App,
    ui::state::{AppState, StateChange},
    utils::throttle::NetworkProcessor,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

type AsyncTaskHandle<T> = tokio::task::JoinHandle<Result<T>>;

#[derive(Debug, Clone)]
pub struct RepoSetupResult {
    pub repo_path: std::path::PathBuf,
    pub branches: Vec<String>,
    pub base_repo_path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WorkItemsResult {
    pub pr_index: usize,
    pub work_items: Vec<WorkItem>,
}

type RepoSetupTaskHandle = AsyncTaskHandle<RepoSetupResult>;
type WorkItemsTaskHandle = AsyncTaskHandle<WorkItemsResult>;

#[derive(Debug, Clone, PartialEq)]
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
    pr_fetch_task: Option<tokio::task::JoinHandle<Result<Vec<PullRequest>>>>,
    repo_setup_task: Option<RepoSetupTaskHandle>,
    git_history_task: Option<tokio::task::JoinHandle<Result<crate::git::CommitHistory>>>,
    work_items_tasks: Option<Vec<WorkItemsTaskHandle>>,
    analysis_task: Option<tokio::task::JoinHandle<Result<crate::models::MigrationAnalysis>>>,
    network_processor: Option<NetworkProcessor>,

    // Progress tracking
    total_prs: usize,
    work_items_fetched: usize,
    work_items_total: usize,
    prs_analyzed: usize,
    prs_to_analyze: usize,
    analysis_progress: Option<Arc<AtomicUsize>>,
    migration_id: String,

    // Intermediate results
    prs: Vec<PullRequest>,
    prs_with_work_items: Vec<PullRequestWithWorkItems>,
    repo_path: Option<std::path::PathBuf>,
    base_repo_path: Option<std::path::PathBuf>,
    terminal_states: Option<Vec<String>>,
    commit_history: Option<crate::git::CommitHistory>,
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
            git_history_task: None,
            work_items_tasks: None,
            analysis_task: None,
            network_processor: None,
            total_prs: 0,
            work_items_fetched: 0,
            work_items_total: 0,
            migration_id: format!("migration-{}", timestamp),
            prs_analyzed: 0,
            prs_to_analyze: 0,
            analysis_progress: None,
            prs: Vec::new(),
            prs_with_work_items: Vec::new(),
            repo_path: None,
            base_repo_path: None,
            terminal_states: None,
            commit_history: None,
        }
    }

    async fn start_pr_fetching(&mut self, app: &App) -> Result<()> {
        self.loading_stage = LoadingStage::FetchingPullRequests;
        self.status = "Fetching pull requests...".to_string();
        self.progress = 0.1;

        let client = app.client().clone();
        let dev_branch = app.dev_branch().to_string();
        let since = app.since().map(|s| s.to_string());

        self.pr_fetch_task = Some(tokio::spawn(async move {
            let prs = client
                .fetch_pull_requests(&dev_branch, since.as_deref())
                .await
                .context("Failed to fetch pull requests")?;

            // For migration mode, we want all PRs, not just untagged ones
            Ok(prs)
        }));

        Ok(())
    }

    async fn check_pr_fetch_progress(&mut self) -> Result<Option<Vec<PullRequest>>> {
        if let Some(task) = &mut self.pr_fetch_task
            && task.is_finished()
        {
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
                    return Err(e).context("PR fetch task failed");
                }
            }
        }
        Ok(None)
    }

    async fn start_repository_setup(&mut self) -> Result<()> {
        if let Some(config) = &self.config {
            self.loading_stage = LoadingStage::SettingUpRepository;
            self.status = "Setting up repository and preparing git history fetch...".to_string();
            self.progress = 0.2;

            let config_clone = config.clone();
            let migration_id = self.migration_id.clone();
            self.repo_setup_task = Some(tokio::spawn(async move {
                Self::perform_repository_setup(config_clone, migration_id).await
            }));
        }
        Ok(())
    }

    async fn perform_repository_setup(
        config: AppConfig,
        migration_id: String,
    ) -> Result<RepoSetupResult> {
        // Create client from config
        let client = AzureDevOpsClient::new(
            config.shared().organization.value().clone(),
            config.shared().project.value().clone(),
            config.shared().repository.value().clone(),
            config.shared().pat.value().clone(),
        )
        .context("Failed to create client")?;

        // Setup repository for analysis
        let repo_details = client
            .fetch_repo_details()
            .await
            .context("Failed to fetch repository details")?;

        // If using local repo, attempt to clean up any existing migration worktrees
        if let Some(local_repo) = &config.shared().local_repo {
            // Clean up the old hardcoded migration worktree
            let _ = force_remove_worktree(
                std::path::Path::new(local_repo.value()),
                "migration-analysis",
            );
            // Clean up any timestamped migration worktrees from previous runs
            let _ = cleanup_migration_worktrees(std::path::Path::new(local_repo.value()));
        }

        let repo_setup = setup_repository(
            config
                .shared()
                .local_repo
                .as_ref()
                .map(|p| p.value().as_str()),
            &repo_details.ssh_url,
            config.shared().target_branch.value(),
            &migration_id,
        )
        .context("Failed to setup repository")?;

        let (repo_path, base_repo_path) = match &repo_setup {
            crate::git::RepositorySetup::Local(path) => (
                path.to_path_buf(),
                config
                    .shared()
                    .local_repo
                    .as_ref()
                    .map(|p| std::path::PathBuf::from(p.value())),
            ),
            crate::git::RepositorySetup::Clone(path, _) => (path.to_path_buf(), None),
        };

        // Parse terminal states
        let terminal_states = match &config {
            AppConfig::Migration { migration, .. } => migration.terminal_states.value().clone(),
            _ => bail!("Migration mode should have migration config"),
        };

        Ok(RepoSetupResult {
            repo_path,
            branches: terminal_states,
            base_repo_path,
        })
    }

    async fn check_repository_setup_progress(&mut self) -> Result<bool> {
        if let Some(task) = &mut self.repo_setup_task
            && task.is_finished()
        {
            let task = self.repo_setup_task.take().unwrap();
            match task.await {
                Ok(Ok(result)) => {
                    self.repo_path = Some(result.repo_path.clone());
                    self.base_repo_path = result.base_repo_path;
                    self.terminal_states = Some(result.branches);

                    // Start git history fetch in parallel now that repo is ready
                    if let Some(config) = &self.config {
                        let repo_path_clone = result.repo_path.clone();
                        let target_branch = config.shared().target_branch.clone();

                        self.git_history_task = Some(tokio::spawn(async move {
                            get_target_branch_history(&repo_path_clone, &target_branch)
                                .context("Failed to get target branch history")
                        }));
                    }

                    return Ok(true);
                }
                Ok(Err(e)) => {
                    return Err(e);
                }
                Err(e) => {
                    return Err(e).context("Repository setup task failed");
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

        // Initialize network processor with configurable network and processing throttling
        self.network_processor = Some(NetworkProcessor::new_with_limits(
            app.max_concurrent_network(),
            app.max_concurrent_processing(),
        ));

        // Start all network tasks in parallel without batching
        self.start_all_work_items_fetching(app);
    }

    fn start_all_work_items_fetching(&mut self, app: &App) {
        let mut tasks = Vec::new();

        // Clone the network processor for use in tasks
        let network_processor = self.network_processor.as_ref().unwrap().clone();

        // Start network requests with throttling
        for index in 0..self.prs.len() {
            if let Some(pr) = self.prs.get(index) {
                let client = app.client().clone();
                let pr_id = pr.id;
                let processor = network_processor.clone();

                let task = tokio::spawn(async move {
                    let result = processor
                        .execute_network_operation(|| async {
                            client
                                .fetch_work_items_with_history_for_pr(pr_id)
                                .await
                                .context("Failed to fetch work items")
                        })
                        .await;

                    match result {
                        Ok(work_items) => Ok(WorkItemsResult {
                            pr_index: index,
                            work_items,
                        }),
                        Err(e) => Err(e),
                    }
                });

                tasks.push(task);
            }
        }

        self.work_items_tasks = Some(tasks);
    }

    async fn check_work_items_progress(&mut self, _app: &App) -> Result<bool> {
        if let Some(ref mut tasks) = self.work_items_tasks {
            let mut completed = Vec::new();
            let mut still_running = Vec::new();

            // Check which tasks have completed
            for task in tasks.drain(..) {
                if task.is_finished() {
                    match task.await {
                        Ok(Ok(result)) => {
                            completed.push(result);
                        }
                        Ok(Err(e)) => {
                            return Err(e).context("Failed to fetch work items");
                        }
                        Err(e) => {
                            return Err(e).context("Work items task failed");
                        }
                    }
                } else {
                    still_running.push(task);
                }
            }

            // Update completed work items
            for result in completed {
                if let Some(pr) = self.prs.get(result.pr_index) {
                    self.prs_with_work_items.push(PullRequestWithWorkItems {
                        pr: pr.clone(),
                        work_items: result.work_items,
                        selected: false,
                    });
                    self.work_items_fetched += 1;
                }
            }

            // Update progress
            if self.work_items_total > 0 {
                self.progress =
                    0.3 + (0.3 * self.work_items_fetched as f64 / self.work_items_total as f64);
                self.status = format!(
                    "Fetching work items ({}/{})",
                    self.work_items_fetched, self.work_items_total
                );
            }
            *tasks = still_running;

            // Check if all tasks are completed
            if tasks.is_empty() {
                // All work items fetched
                self.work_items_tasks = None;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(true) // No tasks means we're done
        }
    }

    async fn start_migration_analysis(&mut self) -> Result<()> {
        if !self.prs_with_work_items.is_empty() {
            // Wait for git history fetch to complete if still running
            if let Some(task) = self.git_history_task.take() {
                self.status = "Waiting for git history fetch to complete...".to_string();
                match task.await {
                    Ok(Ok(commit_history)) => {
                        self.commit_history = Some(commit_history);
                    }
                    Ok(Err(e)) => {
                        return Err(e);
                    }
                    Err(e) => {
                        return Err(e).context("Git history fetch task failed");
                    }
                }
            }

            // Ensure we have the commit history
            if self.commit_history.is_none() {
                bail!("Commit history not available for analysis");
            }

            self.loading_stage = LoadingStage::RunningAnalysis;
            self.prs_to_analyze = self.prs_with_work_items.len();
            self.prs_analyzed = 0;

            self.progress = 0.7;

            // Create shared progress counter
            let progress_counter = Arc::new(AtomicUsize::new(0));
            self.analysis_progress = Some(progress_counter.clone());

            let prs_with_work_items = self.prs_with_work_items.clone();
            let repo_path = self.repo_path.clone().unwrap();
            let terminal_states = self.terminal_states.clone().unwrap();
            let commit_history = self.commit_history.clone().unwrap();
            let config = self.config.clone().unwrap();
            let migration_id = self.migration_id.clone();

            self.analysis_task = Some(tokio::spawn(async move {
                Self::perform_migration_analysis(
                    prs_with_work_items,
                    repo_path,
                    terminal_states,
                    commit_history,
                    config,
                    migration_id,
                    progress_counter,
                )
                .await
            }));
        }
        Ok(())
    }

    async fn perform_migration_analysis(
        prs_with_work_items: Vec<PullRequestWithWorkItems>,
        _repo_path: std::path::PathBuf,
        terminal_states: Vec<String>,
        commit_history: crate::git::CommitHistory,
        config: AppConfig,
        migration_id: String,
        progress_counter: Arc<AtomicUsize>,
    ) -> Result<crate::models::MigrationAnalysis> {
        // Create client from config
        let client = AzureDevOpsClient::new(
            config.shared().organization.value().clone(),
            config.shared().project.value().clone(),
            config.shared().repository.value().clone(),
            config.shared().pat.value().clone(),
        )
        .context("Failed to create client")?;

        // Create migration analyzer
        let analyzer = MigrationAnalyzer::new(client, terminal_states);

        // Analyze PRs using pre-fetched commit history (no individual git commands per PR)
        let mut pr_analyses = Vec::new();
        for pr_with_work_items in prs_with_work_items {
            let analysis = analyzer
                .analyze_single_pr(&pr_with_work_items, &commit_history)
                .await
                .with_context(|| format!("Analysis failed for PR {}", pr_with_work_items.pr.id))?;

            pr_analyses.push(analysis);

            // Update progress counter
            progress_counter.fetch_add(1, Ordering::Relaxed);
        }

        // Categorize PRs
        let analysis = analyzer
            .categorize_prs(pr_analyses)
            .context("Failed to categorize PRs")?;

        // Clean up migration worktree
        if let Some(local_repo) = &config.shared().local_repo {
            let _ = force_remove_worktree(std::path::Path::new(local_repo.value()), &migration_id);
        }

        Ok(analysis)
    }

    async fn check_analysis_progress(&mut self, app: &mut App) -> Result<bool> {
        if let Some(task) = &mut self.analysis_task {
            if task.is_finished() {
                let task = self.analysis_task.take().unwrap();
                match task.await {
                    Ok(Ok(analysis)) => {
                        self.loading_stage = LoadingStage::Complete;
                        self.status = "Analysis complete!".to_string();
                        self.progress = 1.0;
                        app.set_migration_analysis(Some(analysis));
                        // Worktree cleanup is now handled by WorktreeContext on exit
                        return Ok(true);
                    }
                    Ok(Err(e)) => {
                        return Err(e);
                    }
                    Err(e) => {
                        return Err(e).context("Analysis task failed");
                    }
                }
            }

            // Update progress while analysis is running
            if let Some(ref progress_counter) = self.analysis_progress {
                self.prs_analyzed = progress_counter.load(Ordering::Relaxed);
            }

            // Calculate progress based on analyzed PRs
            if self.prs_to_analyze > 0 {
                let base_progress = 0.7; // Starting progress for analysis phase
                let analysis_progress =
                    (self.prs_analyzed as f64 / self.prs_to_analyze as f64) * 0.25; // 25% of total progress for analysis
                self.progress = base_progress + analysis_progress;
            }
        }
        Ok(false)
    }

    fn get_loading_message(&self) -> String {
        match self.loading_stage {
            LoadingStage::NotStarted => "Initializing...".to_string(),
            LoadingStage::FetchingPullRequests => {
                if self.git_history_task.is_some() {
                    "Fetching pull requests and git history...".to_string()
                } else {
                    "Fetching pull requests...".to_string()
                }
            }
            LoadingStage::SettingUpRepository => {
                if self.git_history_task.is_some() {
                    "Setting up repository and fetching git history...".to_string()
                } else {
                    "Setting up repository...".to_string()
                }
            }
            LoadingStage::FetchingWorkItems => {
                let base_msg = if self.work_items_total > 0 {
                    format!(
                        "Fetching work items ({}/{})",
                        self.work_items_fetched, self.work_items_total
                    )
                } else {
                    "Fetching work items...".to_string()
                };

                if self.git_history_task.is_some() {
                    format!("{} and git history...", base_msg)
                } else {
                    base_msg
                }
            }
            LoadingStage::WaitingForWorkItems => {
                let work_items_msg = format!(
                    "Fetching work items ({}/{})",
                    self.work_items_fetched, self.work_items_total
                );

                if self.git_history_task.is_some() {
                    format!("{} and git history...", work_items_msg)
                } else {
                    work_items_msg
                }
            }
            LoadingStage::RunningAnalysis => {
                format!(
                    "Analyzing {}/{} PRs...",
                    self.prs_analyzed, self.prs_to_analyze
                )
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
                self.error = Some(e.to_string());
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
                                self.error = Some(e.to_string());
                            }
                        }
                        Ok(None) => {
                            // Still fetching, continue
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::SettingUpRepository => {
                    match self.check_repository_setup_progress().await {
                        Ok(true) => {
                            // Repository setup complete
                            // Worktree cleanup is now handled by WorktreeContext
                            // Start fetching work items
                            self.start_work_items_fetching(app);
                        }
                        Ok(false) => {
                            // Still setting up, continue
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingWorkItems => {
                    self.loading_stage = LoadingStage::WaitingForWorkItems;
                    return StateChange::Keep;
                }
                LoadingStage::WaitingForWorkItems => {
                    match self.check_work_items_progress(app).await {
                        Ok(true) => {
                            // Work items complete, start migration analysis
                            if let Err(e) = self.start_migration_analysis().await {
                                self.error = Some(e.to_string());
                            }
                        }
                        Ok(false) => {
                            // Still fetching work items, continue
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
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
                            self.error = Some(e.to_string());
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
                // Clean up any existing worktree before retry
                app.cleanup_migration_worktree();

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
                self.base_repo_path = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// # Parallel Execution Flow
    ///
    /// Tests the parallel execution flow for data loading operations.
    ///
    /// ## Test Scenario
    /// - Sets up concurrent data loading tasks
    /// - Tests coordination and synchronization of parallel operations
    ///
    /// ## Expected Outcome
    /// - Parallel operations execute correctly without conflicts
    /// - Data loading flow maintains consistency across concurrent tasks
    #[test]
    fn test_parallel_execution_flow() {
        // This test verifies the parallel execution flow structure
        let config = AppConfig::Migration {
            shared: crate::models::SharedConfig {
                organization: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                project: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                repository: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                pat: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                target_branch: crate::parsed_property::ParsedProperty::Default("main".to_string()),
                dev_branch: crate::parsed_property::ParsedProperty::Default("dev".to_string()),
                local_repo: None,
                max_concurrent_network: crate::parsed_property::ParsedProperty::Default(5),
                max_concurrent_processing: crate::parsed_property::ParsedProperty::Default(2),
                parallel_limit: crate::parsed_property::ParsedProperty::Default(5),
                tag_prefix: crate::parsed_property::ParsedProperty::Default("merged-".to_string()),
                since: None,
                skip_confirmation: false,
            },
            migration: crate::models::MigrationModeConfig {
                terminal_states: crate::parsed_property::ParsedProperty::Default(vec![
                    "Done".to_string(),
                    "Closed".to_string(),
                ]),
            },
        };

        let state = MigrationDataLoadingState::new(config);

        // Initially git_history_task should be None
        assert!(state.git_history_task.is_none());
        assert!(state.commit_history.is_none());

        // Verify initial state
        assert_eq!(state.loading_stage, LoadingStage::NotStarted);
        assert!(state.repo_path.is_none());
        assert!(state.terminal_states.is_none());
    }

    /// # Loading Messages Reflect Parallel Operations
    ///
    /// Tests that loading status messages accurately reflect parallel operation progress.
    ///
    /// ## Test Scenario
    /// - Starts multiple parallel loading operations
    /// - Monitors status messages and progress indicators
    ///
    /// ## Expected Outcome
    /// - Status messages accurately reflect current loading state
    /// - Progress indicators correctly represent parallel operation completion
    #[tokio::test]
    async fn test_loading_messages_reflect_parallel_operations() {
        let config = AppConfig::Migration {
            shared: crate::models::SharedConfig {
                organization: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                project: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                repository: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                pat: crate::parsed_property::ParsedProperty::Default("test".to_string()),
                target_branch: crate::parsed_property::ParsedProperty::Default("main".to_string()),
                dev_branch: crate::parsed_property::ParsedProperty::Default("dev".to_string()),
                local_repo: None,
                max_concurrent_network: crate::parsed_property::ParsedProperty::Default(5),
                max_concurrent_processing: crate::parsed_property::ParsedProperty::Default(2),
                parallel_limit: crate::parsed_property::ParsedProperty::Default(5),
                tag_prefix: crate::parsed_property::ParsedProperty::Default("merged-".to_string()),
                since: None,
                skip_confirmation: false,
            },
            migration: crate::models::MigrationModeConfig {
                terminal_states: crate::parsed_property::ParsedProperty::Default(vec![
                    "Done".to_string(),
                    "Closed".to_string(),
                ]),
            },
        };

        let mut state = MigrationDataLoadingState::new(config);

        // Test that loading messages change when git history task is present
        state.loading_stage = LoadingStage::FetchingPullRequests;
        let msg_without_git = state.get_loading_message();
        assert_eq!(msg_without_git, "Fetching pull requests...");

        // Simulate git history task being started
        state.git_history_task = Some(tokio::spawn(async {
            Ok(crate::git::CommitHistory {
                commit_hashes: std::collections::HashSet::new(),
                commit_messages: Vec::new(),
                commit_bodies: Vec::new(),
            })
        }));

        let msg_with_git = state.get_loading_message();
        assert_eq!(msg_with_git, "Fetching pull requests and git history...");
    }

    /// # Migration Data Loading - Initial State
    ///
    /// Tests the migration data loading screen in initial state.
    ///
    /// ## Test Scenario
    /// - Creates a new migration data loading state
    /// - Renders without starting any loading operations
    ///
    /// ## Expected Outcome
    /// - Should display initializing message
    /// - Should show progress bar at 0%
    #[test]
    fn test_migration_data_loading_initial() {
        use crate::ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_migration},
        };
        use insta::assert_snapshot;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config.clone());

            let state = Box::new(MigrationDataLoadingState::new(config));
            harness.render_state(state);

            assert_snapshot!("initial", harness.backend());
        });
    }

    /// # Migration Data Loading - Fetching PRs
    ///
    /// Tests the loading screen when fetching pull requests.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state
    /// - Sets stage to fetching pull requests
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Fetching pull requests" message
    /// - Should show progress bar
    #[test]
    fn test_migration_data_loading_fetching_prs() {
        use crate::ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_migration},
        };
        use insta::assert_snapshot;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config.clone());

            let mut state = MigrationDataLoadingState::new(config);
            state.loading_stage = LoadingStage::FetchingPullRequests;
            harness.render_state(Box::new(state));

            assert_snapshot!("fetching_prs", harness.backend());
        });
    }

    /// # Migration Data Loading - Analyzing
    ///
    /// Tests the loading screen during migration analysis.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state
    /// - Sets stage to analyzing for migration
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Analyzing for migration" message
    /// - Should show progress bar
    #[test]
    fn test_migration_data_loading_analyzing() {
        use crate::ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_migration},
        };
        use insta::assert_snapshot;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config.clone());

            let mut state = MigrationDataLoadingState::new(config);
            state.loading_stage = LoadingStage::RunningAnalysis;
            harness.render_state(Box::new(state));

            assert_snapshot!("analyzing", harness.backend());
        });
    }

    /// Helper to create a test app for async tests
    fn create_test_app(config: AppConfig) -> App {
        let client = crate::api::AzureDevOpsClient::new(
            "test-org".to_string(),
            "test-project".to_string(),
            "test-repo".to_string(),
            "test-pat".to_string(),
        )
        .unwrap();
        App::new(Vec::new(), std::sync::Arc::new(config), client)
    }

    /// # Quit Command Returns Exit
    ///
    /// Tests that pressing 'q' at any loading stage returns StateChange::Exit.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state
    /// - Calls process_key with 'q' character
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit immediately
    #[tokio::test]
    async fn test_quit_command_returns_exit() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config.clone());
        let mut app = create_test_app(config);

        // Test quit at various stages
        state.loading_stage = LoadingStage::NotStarted;
        let result = state.process_key(KeyCode::Char('q'), &mut app).await;
        assert!(matches!(result, StateChange::Exit));

        state.loading_stage = LoadingStage::FetchingPullRequests;
        let result = state.process_key(KeyCode::Char('q'), &mut app).await;
        assert!(matches!(result, StateChange::Exit));

        state.loading_stage = LoadingStage::RunningAnalysis;
        let result = state.process_key(KeyCode::Char('q'), &mut app).await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Retry Resets State When Error Exists
    ///
    /// Tests that pressing 'r' when an error exists resets all state for retry.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state with an error
    /// - Calls process_key with 'r' character
    ///
    /// ## Expected Outcome
    /// - Error should be cleared
    /// - Loading stage should reset to NotStarted
    /// - Progress should reset to 0
    /// - All task handles should be cleared
    #[tokio::test]
    async fn test_retry_resets_state_when_error_exists() {
        use crate::ui::testing::{create_test_config_migration, create_test_pull_request};

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config.clone());

        // Set up error state
        state.error = Some("Test error".to_string());
        state.loading_stage = LoadingStage::FetchingPullRequests;
        state.progress = 0.5;
        state.loaded = true;
        state.total_prs = 10;
        state.work_items_fetched = 5;
        state.prs = vec![create_test_pull_request()];

        let mut app = create_test_app(config);

        // Press 'r' to retry
        let result = state.process_key(KeyCode::Char('r'), &mut app).await;

        // Verify state was reset
        assert!(matches!(result, StateChange::Keep));
        assert!(state.error.is_none());
        assert_eq!(state.loading_stage, LoadingStage::NotStarted);
        assert_eq!(state.progress, 0.0);
        assert!(!state.loaded);
        assert_eq!(state.total_prs, 0);
        assert_eq!(state.work_items_fetched, 0);
        assert!(state.prs.is_empty());
        assert!(state.prs_with_work_items.is_empty());
        assert!(state.repo_path.is_none());
        assert!(state.terminal_states.is_none());
    }

    /// # Retry Ignored Without Error
    ///
    /// Tests that pressing 'r' without an error does nothing.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state without error
    /// - Calls process_key with 'r' character
    ///
    /// ## Expected Outcome
    /// - State should remain unchanged
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_retry_ignored_without_error() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config.clone());

        // No error set
        state.loading_stage = LoadingStage::FetchingPullRequests;
        state.progress = 0.5;
        state.loaded = true;

        let mut app = create_test_app(config);

        // Press 'r' without error
        let result = state.process_key(KeyCode::Char('r'), &mut app).await;

        // State should be unchanged
        assert!(matches!(result, StateChange::Keep));
        assert_eq!(state.loading_stage, LoadingStage::FetchingPullRequests);
        assert_eq!(state.progress, 0.5);
        assert!(state.loaded);
    }

    /// # Any Key Continues After Completion
    ///
    /// Tests that pressing any key after completion transitions to results.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state at Complete stage
    /// - Calls process_key with various keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to MigrationResultsState
    #[tokio::test]
    async fn test_any_key_continues_after_completion() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config.clone());

        // Set complete state
        state.loading_stage = LoadingStage::Complete;
        state.loaded = true;

        let mut app = create_test_app(config);

        // Press Enter to continue
        let result = state.process_key(KeyCode::Enter, &mut app).await;
        assert!(matches!(result, StateChange::Change(_)));

        // Reset and test with space
        state.loading_stage = LoadingStage::Complete;
        let result = state.process_key(KeyCode::Char(' '), &mut app).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Loading Message for All Stages
    ///
    /// Tests that get_loading_message returns correct message for each stage.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state
    /// - Tests each loading stage
    ///
    /// ## Expected Outcome
    /// - Each stage should have appropriate loading message
    #[test]
    fn test_loading_message_for_all_stages() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config);

        // NotStarted
        state.loading_stage = LoadingStage::NotStarted;
        assert_eq!(state.get_loading_message(), "Initializing...");

        // FetchingPullRequests without git history
        state.loading_stage = LoadingStage::FetchingPullRequests;
        state.git_history_task = None;
        assert_eq!(state.get_loading_message(), "Fetching pull requests...");

        // SettingUpRepository without git history
        state.loading_stage = LoadingStage::SettingUpRepository;
        assert_eq!(state.get_loading_message(), "Setting up repository...");

        // FetchingWorkItems without progress
        state.loading_stage = LoadingStage::FetchingWorkItems;
        state.work_items_total = 0;
        assert_eq!(state.get_loading_message(), "Fetching work items...");

        // FetchingWorkItems with progress
        state.work_items_total = 10;
        state.work_items_fetched = 5;
        assert_eq!(state.get_loading_message(), "Fetching work items (5/10)");

        // WaitingForWorkItems
        state.loading_stage = LoadingStage::WaitingForWorkItems;
        assert_eq!(state.get_loading_message(), "Fetching work items (5/10)");

        // RunningAnalysis
        state.loading_stage = LoadingStage::RunningAnalysis;
        state.prs_analyzed = 3;
        state.prs_to_analyze = 10;
        assert_eq!(state.get_loading_message(), "Analyzing 3/10 PRs...");

        // Complete
        state.loading_stage = LoadingStage::Complete;
        assert_eq!(state.get_loading_message(), "Analysis complete");
    }

    /// # Loading Message With Git History Task
    ///
    /// Tests that loading messages include git history when task is running.
    ///
    /// ## Test Scenario
    /// - Creates a state with git_history_task set
    /// - Tests messages at various stages
    ///
    /// ## Expected Outcome
    /// - Messages should mention git history when task is running
    #[tokio::test]
    async fn test_loading_message_with_git_history_task() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config);

        // Set up a git history task
        state.git_history_task = Some(tokio::spawn(async {
            Ok(crate::git::CommitHistory {
                commit_hashes: std::collections::HashSet::new(),
                commit_messages: Vec::new(),
                commit_bodies: Vec::new(),
            })
        }));

        // FetchingPullRequests with git history
        state.loading_stage = LoadingStage::FetchingPullRequests;
        assert_eq!(
            state.get_loading_message(),
            "Fetching pull requests and git history..."
        );

        // SettingUpRepository with git history
        state.loading_stage = LoadingStage::SettingUpRepository;
        assert_eq!(
            state.get_loading_message(),
            "Setting up repository and fetching git history..."
        );

        // FetchingWorkItems with git history
        state.loading_stage = LoadingStage::FetchingWorkItems;
        state.work_items_total = 10;
        state.work_items_fetched = 5;
        assert_eq!(
            state.get_loading_message(),
            "Fetching work items (5/10) and git history..."
        );

        // WaitingForWorkItems with git history
        state.loading_stage = LoadingStage::WaitingForWorkItems;
        assert_eq!(
            state.get_loading_message(),
            "Fetching work items (5/10) and git history..."
        );
    }

    /// # Initial Load Trigger Sets Loading Stage
    ///
    /// Tests that the first KeyCode::Null triggers PR fetching.
    ///
    /// ## Test Scenario
    /// - Creates a fresh state with loaded = false
    /// - Calls process_key with KeyCode::Null
    ///
    /// ## Expected Outcome
    /// - loaded flag should be set to true
    /// - PR fetch task should be started
    /// - Returns StateChange::Keep
    #[tokio::test]
    async fn test_initial_load_trigger() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config.clone());

        // Verify initial state
        assert!(!state.loaded);
        assert_eq!(state.loading_stage, LoadingStage::NotStarted);

        let mut app = create_test_app(config);

        // Trigger initial load
        let result = state.process_key(KeyCode::Null, &mut app).await;

        // Verify state after trigger
        assert!(matches!(result, StateChange::Keep));
        assert!(state.loaded);
        assert_eq!(state.loading_stage, LoadingStage::FetchingPullRequests);
        assert!(state.pr_fetch_task.is_some());
    }

    /// # Complete Stage Transitions to Results on Null
    ///
    /// Tests that KeyCode::Null at Complete stage transitions to results.
    ///
    /// ## Test Scenario
    /// - Creates a state at Complete stage
    /// - Calls process_key with KeyCode::Null
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to MigrationResultsState
    #[tokio::test]
    async fn test_complete_stage_transitions_on_null() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config.clone());

        // Set to complete and loaded
        state.loading_stage = LoadingStage::Complete;
        state.loaded = true;

        let mut app = create_test_app(config);

        // KeyCode::Null at Complete should transition
        let result = state.process_key(KeyCode::Null, &mut app).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Migration Data Loading - Error State
    ///
    /// Tests the loading screen when an error occurs.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state with error
    /// - Renders the error display
    ///
    /// ## Expected Outcome
    /// - Should display error message in red
    /// - Should show retry help text
    #[test]
    fn test_migration_data_loading_error_state() {
        use crate::ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_migration},
        };
        use insta::assert_snapshot;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config.clone());

            let mut state = MigrationDataLoadingState::new(config);
            state.error = Some("Failed to connect to Azure DevOps API".to_string());
            state.loading_stage = LoadingStage::FetchingPullRequests;
            harness.render_state(Box::new(state));

            assert_snapshot!("error_state", harness.backend());
        });
    }

    /// # Migration Data Loading - Complete State
    ///
    /// Tests the loading screen when analysis is complete.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state at Complete stage
    /// - Renders the completion display
    ///
    /// ## Expected Outcome
    /// - Should display completion message
    /// - Should show 100% progress
    /// - Should show "press any key" help text
    #[test]
    fn test_migration_data_loading_complete_state() {
        use crate::ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_migration},
        };
        use insta::assert_snapshot;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config.clone());

            let mut state = MigrationDataLoadingState::new(config);
            state.loading_stage = LoadingStage::Complete;
            state.progress = 1.0;
            harness.render_state(Box::new(state));

            assert_snapshot!("complete_state", harness.backend());
        });
    }

    /// # Migration Data Loading - Work Items Progress
    ///
    /// Tests the loading screen during work items fetching.
    ///
    /// ## Test Scenario
    /// - Creates a migration data loading state at WaitingForWorkItems
    /// - Sets progress values
    /// - Renders the progress display
    ///
    /// ## Expected Outcome
    /// - Should display work items progress (x/y)
    /// - Should show appropriate progress percentage
    #[test]
    fn test_migration_data_loading_work_items_progress() {
        use crate::ui::{
            snapshot_testing::with_settings_and_module_path,
            testing::{TuiTestHarness, create_test_config_migration},
        };
        use insta::assert_snapshot;

        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_migration();
            let mut harness = TuiTestHarness::with_config(config.clone());

            let mut state = MigrationDataLoadingState::new(config);
            state.loading_stage = LoadingStage::WaitingForWorkItems;
            state.work_items_total = 25;
            state.work_items_fetched = 12;
            state.progress = 0.45;
            harness.render_state(Box::new(state));

            assert_snapshot!("work_items_progress", harness.backend());
        });
    }

    /// # NotStarted Stage Handling
    ///
    /// Tests that NotStarted stage handles KeyCode::Null gracefully.
    ///
    /// ## Test Scenario
    /// - Creates a state at NotStarted with loaded = true
    /// - Calls process_key with KeyCode::Null
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep without error
    #[tokio::test]
    async fn test_not_started_stage_handling() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let mut state = MigrationDataLoadingState::new(config.clone());

        // Edge case: loaded but still NotStarted
        state.loaded = true;
        state.loading_stage = LoadingStage::NotStarted;

        let mut app = create_test_app(config);

        // Should handle gracefully
        let result = state.process_key(KeyCode::Null, &mut app).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # State Constructor Initializes Correctly
    ///
    /// Tests that new() sets all fields to correct initial values.
    ///
    /// ## Test Scenario
    /// - Creates a new MigrationDataLoadingState
    /// - Checks all field values
    ///
    /// ## Expected Outcome
    /// - All fields should have correct initial values
    #[test]
    fn test_state_constructor_initializes_correctly() {
        use crate::ui::testing::create_test_config_migration;

        let config = create_test_config_migration();
        let state = MigrationDataLoadingState::new(config);

        // Check all initial values
        assert_eq!(state.loading_stage, LoadingStage::NotStarted);
        assert!(!state.loaded);
        assert_eq!(state.status, "Initializing migration analysis...");
        assert_eq!(state.progress, 0.0);
        assert!(state.error.is_none());
        assert!(state.config.is_some());
        assert!(state.pr_fetch_task.is_none());
        assert!(state.repo_setup_task.is_none());
        assert!(state.git_history_task.is_none());
        assert!(state.work_items_tasks.is_none());
        assert!(state.analysis_task.is_none());
        assert!(state.network_processor.is_none());
        assert_eq!(state.total_prs, 0);
        assert_eq!(state.work_items_fetched, 0);
        assert_eq!(state.work_items_total, 0);
        assert_eq!(state.prs_analyzed, 0);
        assert_eq!(state.prs_to_analyze, 0);
        assert!(state.analysis_progress.is_none());
        assert!(state.migration_id.starts_with("migration-"));
        assert!(state.prs.is_empty());
        assert!(state.prs_with_work_items.is_empty());
        assert!(state.repo_path.is_none());
        assert!(state.terminal_states.is_none());
        assert!(state.commit_history.is_none());
    }
}
