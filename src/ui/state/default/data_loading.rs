use super::PullRequestSelectionState;
use crate::ui::state::shared::ErrorState;
use crate::{
    api,
    models::PullRequestWithWorkItems,
    ui::App,
    ui::state::{AppState, StateChange},
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
}

#[derive(Debug, Clone)]
enum LoadingStage {
    NotStarted,
    FetchingPullRequests,
    FetchingWorkItems,
    WaitingForWorkItems,
    FetchingCommitInfo,
    Complete,
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
        }
    }

    async fn fetch_pull_requests(&mut self, app: &mut App) -> Result<()> {
        self.loading_stage = LoadingStage::FetchingPullRequests;

        // Fetch pull requests
        let prs = match app
            .client
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
        app.pull_requests = filtered_prs
            .into_iter()
            .map(|pr| PullRequestWithWorkItems {
                pr,
                work_items: Vec::new(),
                selected: false,
            })
            .collect();

        Ok(())
    }

    fn start_work_items_fetching(&mut self, app: &App) {
        self.loading_stage = LoadingStage::FetchingWorkItems;
        self.work_items_total = app.pull_requests.len();
        self.work_items_fetched = 0;

        // Use network processor to throttle network operations
        use crate::utils::throttle::NetworkProcessor;

        let network_processor = NetworkProcessor::new_with_limits(
            app.max_concurrent_network(),
            app.max_concurrent_processing(),
        );
        let mut tasks = Vec::new();

        for index in 0..app.pull_requests.len() {
            if let Some(pr_with_wi) = app.pull_requests.get(index) {
                let client = app.client.clone();
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

    async fn check_work_items_progress(&mut self, app: &mut App) -> Result<bool> {
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
                if let Some(pr_with_wi) = app.pull_requests.get_mut(result.pr_index) {
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

    async fn fetch_commit_info(&mut self, app: &mut App) -> Result<()> {
        self.loading_stage = LoadingStage::FetchingCommitInfo;
        self.commit_info_total = app
            .pull_requests
            .iter()
            .filter(|pr| pr.pr.last_merge_commit.is_none())
            .count();
        self.commit_info_fetched = 0;

        for pr_with_wi in &mut app.pull_requests {
            if pr_with_wi.pr.last_merge_commit.is_none() {
                match app.client.fetch_pr_commit(pr_with_wi.pr.id).await {
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

        self.loading_stage = LoadingStage::Complete;
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
            LoadingStage::Complete => "Loading complete".to_string(),
        }
    }
}

#[async_trait]
impl AppState for DataLoadingState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let loading = Paragraph::new(self.get_loading_message())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Loading"))
            .alignment(Alignment::Center);
        f.render_widget(loading, f.area());
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
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
                        app.error_message = Some(e.to_string());
                        return StateChange::Change(Box::new(ErrorState::new()));
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
                                app.error_message = Some(e.to_string());
                                return StateChange::Change(Box::new(ErrorState::new()));
                            }
                        }
                        Ok(false) => {
                            // Still waiting for work items, continue
                        }
                        Err(e) => {
                            app.error_message = Some(e.to_string());
                            return StateChange::Change(Box::new(ErrorState::new()));
                        }
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingCommitInfo => {
                    // Loading is complete, transition to PR selection
                    return StateChange::Change(Box::new(PullRequestSelectionState::new()));
                }
                LoadingStage::Complete => {
                    // Should not reach here, but transition to PR selection just in case
                    return StateChange::Change(Box::new(PullRequestSelectionState::new()));
                }
            }
        }

        // Allow quitting during loading
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }
}
