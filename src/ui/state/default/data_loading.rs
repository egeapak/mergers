use super::PullRequestSelectionState;
use crate::ui::state::shared::ErrorState;
use crate::{
    api,
    core::operations::{DependencyAnalyzer, FileChange, PRInfo},
    git,
    models::PullRequestWithWorkItems,
    ui::apps::MergeApp,
    ui::state::default::MergeState,
    ui::state::typed::{ModeState, StateChange},
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;

type AsyncTaskHandle<T> = tokio::task::JoinHandle<Result<T>>;

#[derive(Debug, Clone)]
pub struct WorkItemsResult {
    pub pr_index: usize,
    pub work_items: Vec<crate::models::WorkItem>,
}

type WorkItemsTaskHandle = AsyncTaskHandle<WorkItemsResult>;

pub struct DataLoadingState {
    loading_stage: LoadingStage,
    loaded: bool,
    work_items_fetched: usize,
    work_items_total: usize,
    commit_info_fetched: usize,
    commit_info_total: usize,
    work_items_tasks: Option<Vec<WorkItemsTaskHandle>>,
    dependency_analysis_total: usize,
}

#[derive(Debug, Clone)]
enum LoadingStage {
    NotStarted,
    FetchingPullRequests,
    FetchingWorkItems,
    WaitingForWorkItems,
    FetchingCommitInfo,
    AnalyzingDependencies,
}

impl Default for DataLoadingState {
    fn default() -> Self {
        Self::new()
    }
}

impl DataLoadingState {
    pub fn new() -> Self {
        Self {
            loading_stage: LoadingStage::NotStarted,
            loaded: false,
            work_items_fetched: 0,
            work_items_total: 0,
            commit_info_fetched: 0,
            commit_info_total: 0,
            work_items_tasks: None,
            dependency_analysis_total: 0,
        }
    }

    async fn fetch_pull_requests(&mut self, app: &mut MergeApp) -> Result<()> {
        self.loading_stage = LoadingStage::FetchingPullRequests;

        // Fetch pull requests
        let prs = match app
            .client()
            .fetch_pull_requests(app.dev_branch(), app.since())
            .await
        {
            Ok(prs) => prs,
            Err(e) => return Err(e).context("Failed to fetch pull requests"),
        };

        let filtered_prs = api::filter_prs_without_merged_tag(prs);

        if filtered_prs.is_empty() {
            bail!("No pull requests found without merged tags.");
        }

        // Initialize PRs with empty work items for now
        *app.pull_requests_mut() = filtered_prs
            .into_iter()
            .map(|pr| PullRequestWithWorkItems {
                pr,
                work_items: Vec::new(),
                selected: false,
            })
            .collect();

        Ok(())
    }

    fn start_work_items_fetching(&mut self, app: &MergeApp) {
        self.loading_stage = LoadingStage::FetchingWorkItems;
        self.work_items_total = app.pull_requests().len();
        self.work_items_fetched = 0;

        // Use network processor to throttle network operations
        use crate::utils::throttle::NetworkProcessor;

        let network_processor = NetworkProcessor::new_with_limits(
            app.max_concurrent_network(),
            app.max_concurrent_processing(),
        );
        let mut tasks = Vec::new();

        for index in 0..app.pull_requests().len() {
            if let Some(pr_with_wi) = app.pull_requests().get(index) {
                let client = app.client().clone();
                let pr_id = pr_with_wi.pr.id;
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

    async fn check_work_items_progress(&mut self, app: &mut MergeApp) -> Result<bool> {
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
                if let Some(pr_with_wi) = app.pull_requests_mut().get_mut(result.pr_index) {
                    pr_with_wi.work_items = result.work_items;
                    self.work_items_fetched += 1;
                }
            }

            *tasks = still_running;

            // Check if all tasks are completed
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

    async fn fetch_commit_info(&mut self, app: &mut MergeApp) -> Result<()> {
        self.loading_stage = LoadingStage::FetchingCommitInfo;
        self.commit_info_total = app
            .pull_requests()
            .iter()
            .filter(|pr| pr.pr.last_merge_commit.is_none())
            .count();
        self.commit_info_fetched = 0;

        let client = app.client().clone();
        for pr_with_wi in app.pull_requests_mut() {
            if pr_with_wi.pr.last_merge_commit.is_none() {
                match client.fetch_pr_commit(pr_with_wi.pr.id).await {
                    Ok(commit_info) => {
                        pr_with_wi.pr.last_merge_commit = Some(commit_info);
                    }
                    Err(e) => {
                        return Err(e).with_context(|| {
                            format!("Failed to fetch commit for PR #{}", pr_with_wi.pr.id)
                        });
                    }
                }
                self.commit_info_fetched += 1;
            }
        }

        // Don't set loading_stage here - let the state machine handle transitions
        Ok(())
    }

    /// Performs dependency analysis using a local repository.
    ///
    /// Uses rayon for parallel file change retrieval and analysis.
    fn analyze_dependencies(&mut self, app: &mut MergeApp) -> Result<()> {
        let local_repo = match app.local_repo() {
            Some(path) => path.to_string(),
            None => {
                // No local repo available, skip analysis
                return Ok(());
            }
        };

        let repo_path = Path::new(&local_repo);
        if !repo_path.exists() {
            // Repo doesn't exist, skip analysis
            return Ok(());
        }

        let prs = app.pull_requests();
        self.dependency_analysis_total = prs.len();

        // Build PRInfo list and sort by closed date (oldest first)
        // The dependency analyzer expects PRs in chronological order
        let mut pr_infos: Vec<PRInfo> = prs
            .iter()
            .map(|pr_with_wi| {
                PRInfo::new(
                    pr_with_wi.pr.id,
                    pr_with_wi.pr.title.clone(),
                    pr_with_wi.selected,
                    pr_with_wi
                        .pr
                        .last_merge_commit
                        .as_ref()
                        .map(|c| c.commit_id.clone()),
                )
            })
            .collect();

        // Sort PRs by closed date (oldest first) for correct dependency analysis
        // Create a map of PR ID to closed date for sorting
        let pr_dates: std::collections::HashMap<i32, Option<String>> = prs
            .iter()
            .map(|pr| (pr.pr.id, pr.pr.closed_date.clone()))
            .collect();

        pr_infos.sort_by(|a, b| {
            let date_a = pr_dates.get(&a.id).and_then(|d| d.as_ref());
            let date_b = pr_dates.get(&b.id).and_then(|d| d.as_ref());

            match (date_a, date_b) {
                (Some(da), Some(db)) => da.cmp(db),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.id.cmp(&b.id), // Fallback to ID if no dates
            }
        });

        // Parallel fetch of file changes for each PR
        let pr_changes: HashMap<i32, Vec<FileChange>> = pr_infos
            .par_iter()
            .filter_map(|pr_info| {
                let commit_id = pr_info.commit_id.as_ref()?;
                match git::get_commit_changes_with_ranges(repo_path, commit_id) {
                    Ok(changes) => Some((pr_info.id, changes)),
                    Err(_) => {
                        // Commit might not exist locally, skip
                        Some((pr_info.id, Vec::new()))
                    }
                }
            })
            .collect();

        // Run parallel dependency analysis
        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze_parallel(&pr_infos, &pr_changes);

        // Store the graph in MergeApp
        app.set_dependency_graph(result.graph);

        Ok(())
    }

    fn get_loading_message(&self) -> String {
        match self.loading_stage {
            LoadingStage::NotStarted => "Initializing...".to_string(),
            LoadingStage::FetchingPullRequests => "Fetching pull requests...".to_string(),
            LoadingStage::FetchingWorkItems => "Starting work items fetch...".to_string(),
            LoadingStage::WaitingForWorkItems => {
                if self.work_items_total > 0 {
                    format!(
                        "Fetching work items ({}/{})",
                        self.work_items_fetched, self.work_items_total
                    )
                } else {
                    "Fetching work items...".to_string()
                }
            }
            LoadingStage::FetchingCommitInfo => {
                if self.commit_info_total > 0 {
                    format!(
                        "Fetching commit information ({}/{})",
                        self.commit_info_fetched, self.commit_info_total
                    )
                } else {
                    "Fetching commit information...".to_string()
                }
            }
            LoadingStage::AnalyzingDependencies => {
                if self.dependency_analysis_total > 0 {
                    format!(
                        "Analyzing dependencies ({} PRs)...",
                        self.dependency_analysis_total
                    )
                } else {
                    "Analyzing dependencies...".to_string()
                }
            }
        }
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for DataLoadingState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, _app: &MergeApp) {
        let loading = Paragraph::new(self.get_loading_message())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Loading"))
            .alignment(Alignment::Center);
        f.render_widget(loading, f.area());
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> StateChange<MergeState> {
        // Start loading on first render
        if !self.loaded && code == KeyCode::Null {
            self.loaded = true;
            return StateChange::Keep;
        }

        // Process loading stages
        if self.loaded && code == KeyCode::Null {
            match self.loading_stage {
                LoadingStage::NotStarted => {
                    if let Err(e) = self.fetch_pull_requests(app).await {
                        app.set_error_message(Some(e.to_string()));
                        return StateChange::Change(MergeState::Error(ErrorState::new()));
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingPullRequests => {
                    // Start parallel work items fetching
                    self.start_work_items_fetching(app);
                    return StateChange::Keep;
                }
                LoadingStage::FetchingWorkItems => {
                    // Transition to waiting for work items
                    self.loading_stage = LoadingStage::WaitingForWorkItems;
                    return StateChange::Keep;
                }
                LoadingStage::WaitingForWorkItems => {
                    // Check progress of work items fetching
                    match self.check_work_items_progress(app).await {
                        Ok(true) => {
                            // All work items fetched, move to commit info
                            if let Err(e) = self.fetch_commit_info(app).await {
                                app.set_error_message(Some(e.to_string()));
                                return StateChange::Change(MergeState::Error(ErrorState::new()));
                            }
                        }
                        Ok(false) => {
                            // Still waiting for work items, continue
                        }
                        Err(e) => {
                            app.set_error_message(Some(e.to_string()));
                            return StateChange::Change(MergeState::Error(ErrorState::new()));
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingCommitInfo => {
                    // Commit info done, move to dependency analysis
                    self.loading_stage = LoadingStage::AnalyzingDependencies;
                    return StateChange::Keep;
                }
                LoadingStage::AnalyzingDependencies => {
                    // Run dependency analysis (uses local_repo if available)
                    if let Err(e) = self.analyze_dependencies(app) {
                        // Log error but don't fail - dependency analysis is optional
                        app.set_error_message(Some(format!(
                            "Dependency analysis failed (non-fatal): {}",
                            e
                        )));
                    }
                    // Transition to PR selection
                    return StateChange::Change(MergeState::PullRequestSelection(
                        PullRequestSelectionState::new(),
                    ));
                }
            }
        }

        // Allow quitting during loading
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "DataLoading"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{TuiTestHarness, create_test_config_default},
    };
    use insta::assert_snapshot;

    /// # Data Loading State - Not Started
    ///
    /// Tests the initial loading state before any operations begin.
    ///
    /// ## Test Scenario
    /// - Creates a new data loading state
    /// - Renders the state before any loading operations start
    ///
    /// ## Expected Outcome
    /// - Should display "Initializing..." message
    /// - Should show bordered loading box
    /// - Text should be centered and yellow
    #[test]
    fn test_data_loading_not_started() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            harness.render_state(&mut state);

            assert_snapshot!("not_started", harness.backend());
        });
    }

    /// # Data Loading State - Fetching Pull Requests
    ///
    /// Tests the loading display when fetching pull requests.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to FetchingPullRequests
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Fetching pull requests..." message
    /// - Should maintain consistent layout
    #[test]
    fn test_data_loading_fetching_prs() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::FetchingPullRequests;
            harness.render_state(&mut state);

            assert_snapshot!("fetching_prs", harness.backend());
        });
    }

    /// # Data Loading State - Fetching Work Items
    ///
    /// Tests the loading display when fetching work items with progress.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to WaitingForWorkItems with progress counters
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Fetching work items (5/10)" progress message
    /// - Should show current progress out of total
    #[test]
    fn test_data_loading_fetching_work_items() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::WaitingForWorkItems;
            state.work_items_fetched = 5;
            state.work_items_total = 10;
            harness.render_state(&mut state);

            assert_snapshot!("fetching_work_items", harness.backend());
        });
    }

    /// # Data Loading State - Fetching Commit Info
    ///
    /// Tests the loading display when fetching commit information.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to FetchingCommitInfo with progress counters
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Fetching commit information (2/3)" progress message
    /// - Should show current progress out of total
    #[test]
    fn test_data_loading_fetching_commit_info() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::FetchingCommitInfo;
            state.commit_info_fetched = 2;
            state.commit_info_total = 3;
            harness.render_state(&mut state);

            assert_snapshot!("fetching_commit_info", harness.backend());
        });
    }

    /// # Data Loading State - Complete
    ///
    /// Tests the loading display when loading is complete.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to Complete
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Loading complete" message
    #[test]
    fn test_data_loading_complete() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::AnalyzingDependencies;
            harness.render_state(&mut state);

            assert_snapshot!("analyzing_dependencies_no_count", harness.backend());
        });
    }

    /// # Data Loading State - Quit During Loading
    ///
    /// Tests that pressing 'q' exits during loading.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_data_loading_quit() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = DataLoadingState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Data Loading State - First Null Key Sets Loaded
    ///
    /// Tests that first Null key starts loading.
    ///
    /// ## Test Scenario
    /// - Creates a new data loading state
    /// - Processes Null key (tick event)
    ///
    /// ## Expected Outcome
    /// - Should set loaded=true
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_data_loading_first_tick() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = DataLoadingState::new();
        assert!(!state.loaded);

        let result =
            ModeState::process_key(&mut state, KeyCode::Null, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(state.loaded);
    }

    /// # Data Loading State - Other Keys Ignored During Loading
    ///
    /// Tests that other keys are ignored during loading.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Processes various keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep for all except 'q'
    #[tokio::test]
    async fn test_data_loading_other_keys_ignored() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = DataLoadingState::new();

        for key in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Char('x'),
        ] {
            let result = ModeState::process_key(&mut state, key, harness.merge_app_mut()).await;
            assert!(matches!(result, StateChange::Keep));
        }
    }

    /// # Data Loading State - Default Trait Implementation
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates DataLoadingState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should match DataLoadingState::new()
    #[test]
    fn test_data_loading_default() {
        let state = DataLoadingState::default();
        assert!(!state.loaded);
        assert_eq!(state.work_items_fetched, 0);
        assert_eq!(state.work_items_total, 0);
    }

    /// # Data Loading State - Starting Work Items
    ///
    /// Tests the state when starting to fetch work items.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to FetchingWorkItems (initial stage)
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Starting work items fetch..." message
    #[test]
    fn test_data_loading_starting_work_items() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::FetchingWorkItems;
            harness.render_state(&mut state);

            assert_snapshot!("starting_work_items", harness.backend());
        });
    }

    /// # Data Loading State - Work Items No Total
    ///
    /// Tests work items loading with zero total (fallback message).
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to WaitingForWorkItems with 0 total
    ///
    /// ## Expected Outcome
    /// - Should display fallback "Fetching work items..." message
    #[test]
    fn test_data_loading_work_items_no_total() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::WaitingForWorkItems;
            state.work_items_total = 0;
            harness.render_state(&mut state);

            assert_snapshot!("work_items_no_total", harness.backend());
        });
    }

    /// # Data Loading State - Commit Info No Total
    ///
    /// Tests commit info loading with zero total (fallback message).
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to FetchingCommitInfo with 0 total
    ///
    /// ## Expected Outcome
    /// - Should display fallback "Fetching commit information..." message
    #[test]
    fn test_data_loading_commit_info_no_total() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::FetchingCommitInfo;
            state.commit_info_total = 0;
            harness.render_state(&mut state);

            assert_snapshot!("commit_info_no_total", harness.backend());
        });
    }

    /// # Data Loading State - FetchingCommitInfo Stage Key Processing
    ///
    /// Tests that pressing Null key in FetchingCommitInfo stage transitions
    /// to AnalyzingDependencies stage.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state in FetchingCommitInfo stage
    /// - Sets loaded=true
    /// - Processes Null key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep and transition to AnalyzingDependencies
    #[tokio::test]
    async fn test_data_loading_fetching_commit_info_transitions() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = DataLoadingState::new();
        state.loaded = true;
        state.loading_stage = LoadingStage::FetchingCommitInfo;

        let result =
            ModeState::process_key(&mut state, KeyCode::Null, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(matches!(
            state.loading_stage,
            LoadingStage::AnalyzingDependencies
        ));
    }

    /// # Data Loading State - AnalyzingDependencies Stage Key Processing
    ///
    /// Tests that pressing Null key in AnalyzingDependencies stage transitions
    /// to PullRequestSelectionState.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state in AnalyzingDependencies stage
    /// - Sets loaded=true
    /// - Processes Null key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to PullRequestSelectionState
    #[tokio::test]
    async fn test_data_loading_analyzing_dependencies_transitions() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = DataLoadingState::new();
        state.loaded = true;
        state.loading_stage = LoadingStage::AnalyzingDependencies;

        let result =
            ModeState::process_key(&mut state, KeyCode::Null, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Data Loading State - Analyzing Dependencies Message
    ///
    /// Tests the loading display when analyzing dependencies.
    ///
    /// ## Test Scenario
    /// - Creates a data loading state
    /// - Sets stage to AnalyzingDependencies with PR count
    /// - Renders the loading display
    ///
    /// ## Expected Outcome
    /// - Should display "Analyzing dependencies (N PRs)..." message
    #[test]
    fn test_data_loading_analyzing_dependencies() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            state.loading_stage = LoadingStage::AnalyzingDependencies;
            state.dependency_analysis_total = 15;
            harness.render_state(&mut state);

            assert_snapshot!("analyzing_dependencies", harness.backend());
        });
    }
}
