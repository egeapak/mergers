use super::PullRequestSelectionState;
use crate::ui::state::shared::ErrorState;
use crate::{
    api,
    models::PullRequestWithWorkItems,
    ui::App,
    ui::state::{AppState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

pub struct DataLoadingState {
    loading_stage: LoadingStage,
    loaded: bool,
    work_items_fetched: usize,
    work_items_total: usize,
    commit_info_fetched: usize,
    commit_info_total: usize,
    work_items_tasks:
        Option<Vec<tokio::task::JoinHandle<Result<(usize, Vec<crate::models::WorkItem>), String>>>>,
    current_batch_start: usize,
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
            current_batch_start: 0,
        }
    }

    async fn fetch_pull_requests(&mut self, app: &mut App) -> Result<(), String> {
        self.loading_stage = LoadingStage::FetchingPullRequests;

        // Fetch pull requests
        let prs = match app.client.fetch_pull_requests(&app.dev_branch).await {
            Ok(prs) => prs,
            Err(e) => return Err(format!("Failed to fetch pull requests: {}", e)),
        };

        let filtered_prs = api::filter_prs_without_merged_tag(prs);

        if filtered_prs.is_empty() {
            return Err("No pull requests found without merged tags.".to_string());
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
        self.current_batch_start = 0;

        // Start first batch of parallel tasks for fetching work items
        self.start_next_batch(app);
    }

    fn start_next_batch(&mut self, app: &App) {
        const BATCH_SIZE: usize = 100;
        let mut tasks = Vec::new();

        let end = std::cmp::min(
            self.current_batch_start + BATCH_SIZE,
            app.pull_requests.len(),
        );

        for index in self.current_batch_start..end {
            if let Some(pr_with_wi) = app.pull_requests.get(index) {
                let client = app.client.clone();
                let pr_id = pr_with_wi.pr.id;

                let task = tokio::spawn(async move {
                    let work_items = client
                        .fetch_work_items_with_history_for_pr(pr_id)
                        .await
                        .unwrap_or_default();
                    Ok((index, work_items))
                });

                tasks.push(task);
            }
        }

        self.work_items_tasks = Some(tasks);
    }

    async fn check_work_items_progress(&mut self, app: &mut App) -> Result<bool, String> {
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
                            return Err(format!("Task failed: {}", e));
                        }
                    }
                } else {
                    still_running.push(task);
                }
            }

            // Update completed work items
            for (index, work_items) in completed {
                if let Some(pr_with_wi) = app.pull_requests.get_mut(index) {
                    pr_with_wi.work_items = work_items;
                    self.work_items_fetched += 1;
                }
            }

            *tasks = still_running;

            // Check if current batch is completed
            if tasks.is_empty() {
                const BATCH_SIZE: usize = 100;
                self.current_batch_start += BATCH_SIZE;

                // Check if there are more PRs to process
                if self.current_batch_start < app.pull_requests.len() {
                    // Start next batch
                    self.start_next_batch(app);
                    Ok(false)
                } else {
                    // All batches completed
                    self.work_items_tasks = None;
                    Ok(true)
                }
            } else {
                Ok(false)
            }
        } else {
            Ok(true) // No tasks means we're done
        }
    }

    async fn fetch_commit_info(&mut self, app: &mut App) -> Result<(), String> {
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
                        return Err(format!(
                            "Failed to fetch commit for PR #{}: {}",
                            pr_with_wi.pr.id, e
                        ));
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
                        app.error_message = Some(e);
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
                                app.error_message = Some(e);
                                return StateChange::Change(Box::new(ErrorState::new()));
                            }
                        }
                        Ok(false) => {
                            // Still waiting for work items, continue
                        }
                        Err(e) => {
                            app.error_message = Some(e);
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
