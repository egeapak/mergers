use super::PullRequestSelectionState;
use crate::{
    api,
    core::operations::{DependencyAnalyzer, FileChange, PRDependencyGraph, PRInfo},
    git,
    models::PullRequestWithWorkItems,
    ui::apps::MergeApp,
    ui::state::default::MergeState,
    ui::state::typed::{ModeState, StateChange},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::mpsc;

// ============================================================================
// Channel-Based Message Types
// ============================================================================

/// Messages sent from the background loading task to the UI.
#[derive(Debug, Clone)]
pub enum LoadingProgressMessage {
    /// A step has started executing
    StepStarted(LoadingStep),
    /// A step completed successfully with optional result data
    StepCompleted(LoadingStep, LoadingStepResult),
    /// Progress update within a step (for parallel operations like work items fetch)
    StepProgress(LoadingStep, usize, usize),
    /// All steps completed successfully
    AllComplete,
    /// An error occurred during loading
    Error(LoadingError),
}

/// Result data from completing a loading step.
#[derive(Debug, Clone, Default)]
pub struct LoadingStepResult {
    /// Number of PRs fetched (FetchPullRequests step)
    pub prs_fetched: Option<usize>,
    /// Work items update for a specific PR (FetchWorkItems step)
    pub work_items_update: Option<WorkItemsResult>,
    /// Number of commits fetched (FetchCommitInfo step)
    pub commits_fetched: Option<usize>,
    /// Whether dependency analysis completed (AnalyzeDependencies step)
    /// Reserved for future step result tracking
    #[allow(dead_code)]
    pub dependencies_analyzed: Option<bool>,
}

/// Error types that can occur during data loading.
#[derive(Debug, Clone)]
pub enum LoadingError {
    /// No pull requests found matching criteria (non-recoverable, config issue)
    NoPullRequestsFound,
    /// API request failed (retryable)
    ApiError(String),
    /// Network timeout (retryable) - reserved for future use
    #[allow(dead_code)]
    NetworkTimeout(String),
    /// Local repository not found for dependency analysis (skippable)
    LocalRepoNotFound(String),
    /// Generic error
    Other(String),
}

impl LoadingError {
    /// Returns whether this error can be recovered by retrying
    pub fn can_retry(&self) -> bool {
        matches!(
            self,
            LoadingError::ApiError(_) | LoadingError::NetworkTimeout(_) | LoadingError::Other(_)
        )
    }

    /// Returns whether this error can be skipped (for optional steps)
    pub fn can_skip(&self) -> bool {
        matches!(self, LoadingError::LocalRepoNotFound(_))
    }

    /// Returns the error message for display
    pub fn message(&self) -> String {
        match self {
            LoadingError::NoPullRequestsFound => {
                "No pull requests found matching the specified criteria.\n\n\
                 This might indicate:\n\
                 • No PRs exist on the development branch\n\
                 • All PRs already have merged tags\n\
                 • The date filter excludes all PRs\n\n\
                 Options:\n\
                   • Press 'Esc' to go back and check configuration"
                    .to_string()
            }
            LoadingError::ApiError(msg) => {
                format!(
                    "API request failed: {}\n\n\
                     Options:\n\
                       • Press 'r' to retry\n\
                       • Press 'Esc' to go back",
                    msg
                )
            }
            LoadingError::NetworkTimeout(msg) => {
                format!(
                    "Network timeout: {}\n\n\
                     Options:\n\
                       • Press 'r' to retry\n\
                       • Press 'Esc' to go back",
                    msg
                )
            }
            LoadingError::LocalRepoNotFound(path) => {
                format!(
                    "Local repository not found: {}\n\n\
                     Dependency analysis requires a local repository clone.\n\
                     This step is optional and can be skipped.\n\n\
                     Options:\n\
                       • Press 's' to skip dependency analysis\n\
                       • Press 'Esc' to go back",
                    path
                )
            }
            LoadingError::Other(msg) => {
                format!(
                    "Error: {}\n\n\
                     Options:\n\
                       • Press 'r' to retry\n\
                       • Press 'Esc' to go back",
                    msg
                )
            }
        }
    }
}

// ============================================================================
// Loading Step Definitions
// ============================================================================

/// Represents the individual steps in the data loading wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingStep {
    /// Fetch pull requests from Azure DevOps API
    FetchPullRequests,
    /// Fetch work items for each PR (parallel operation)
    FetchWorkItems,
    /// Fetch commit information for PRs missing it
    FetchCommitInfo,
    /// Analyze file dependencies using local repository (optional)
    AnalyzeDependencies,
}

impl LoadingStep {
    /// Returns the display name for this step
    pub fn display_name(&self) -> &'static str {
        match self {
            LoadingStep::FetchPullRequests => "Fetch PRs",
            LoadingStep::FetchWorkItems => "Work Items",
            LoadingStep::FetchCommitInfo => "Commit Info",
            LoadingStep::AnalyzeDependencies => "Dependencies",
        }
    }

    /// Returns the progress message for this step
    pub fn progress_message(&self, fetched: usize, total: usize) -> String {
        match self {
            LoadingStep::FetchPullRequests => "Fetching pull requests...".to_string(),
            LoadingStep::FetchWorkItems => {
                if total > 0 {
                    format!("Fetching work items ({}/{})...", fetched, total)
                } else {
                    "Fetching work items...".to_string()
                }
            }
            LoadingStep::FetchCommitInfo => {
                if total > 0 {
                    format!("Fetching commit info ({}/{})...", fetched, total)
                } else {
                    "Fetching commit information...".to_string()
                }
            }
            LoadingStep::AnalyzeDependencies => {
                if total > 0 {
                    format!("Analyzing dependencies ({} PRs)...", total)
                } else {
                    "Analyzing dependencies...".to_string()
                }
            }
        }
    }
}

/// Status of a loading step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
}

// ============================================================================
// Progress Tracking
// ============================================================================

/// Tracks progress through the loading wizard steps
#[derive(Debug, Clone)]
pub struct LoadingProgress {
    /// Status of fetch pull requests step
    fetch_pull_requests: StepStatus,
    /// Status of fetch work items step
    fetch_work_items: StepStatus,
    /// Status of fetch commit info step
    fetch_commit_info: StepStatus,
    /// Status of analyze dependencies step
    analyze_dependencies: StepStatus,

    /// Current step being executed
    current_step: Option<LoadingStep>,

    /// Progress counters for parallel operations
    work_items_fetched: usize,
    work_items_total: usize,
    commits_fetched: usize,
    commits_total: usize,
    prs_for_analysis: usize,

    /// Whether dependency analysis is available (local repo exists)
    /// Reserved for future use in dynamic step configuration
    #[allow(dead_code)]
    dependency_analysis_available: bool,
}

impl LoadingProgress {
    /// Creates a new loading progress tracker
    pub fn new(dependency_analysis_available: bool) -> Self {
        Self {
            fetch_pull_requests: StepStatus::Pending,
            fetch_work_items: StepStatus::Pending,
            fetch_commit_info: StepStatus::Pending,
            analyze_dependencies: if dependency_analysis_available {
                StepStatus::Pending
            } else {
                StepStatus::Skipped
            },
            current_step: None,
            work_items_fetched: 0,
            work_items_total: 0,
            commits_fetched: 0,
            commits_total: 0,
            prs_for_analysis: 0,
            dependency_analysis_available,
        }
    }

    /// Returns the list of steps with their status
    pub fn steps(&self) -> Vec<(LoadingStep, StepStatus)> {
        let mut steps = vec![
            (LoadingStep::FetchPullRequests, self.fetch_pull_requests),
            (LoadingStep::FetchWorkItems, self.fetch_work_items),
            (LoadingStep::FetchCommitInfo, self.fetch_commit_info),
        ];
        // Always show dependencies step (will show as skipped if not available)
        steps.push((LoadingStep::AnalyzeDependencies, self.analyze_dependencies));
        steps
    }

    /// Sets a step to in-progress status
    pub fn start_step(&mut self, step: LoadingStep) {
        self.current_step = Some(step);
        match step {
            LoadingStep::FetchPullRequests => self.fetch_pull_requests = StepStatus::InProgress,
            LoadingStep::FetchWorkItems => self.fetch_work_items = StepStatus::InProgress,
            LoadingStep::FetchCommitInfo => self.fetch_commit_info = StepStatus::InProgress,
            LoadingStep::AnalyzeDependencies => {
                self.analyze_dependencies = StepStatus::InProgress;
            }
        }
    }

    /// Marks a step as completed
    pub fn complete_step(&mut self, step: LoadingStep) {
        match step {
            LoadingStep::FetchPullRequests => self.fetch_pull_requests = StepStatus::Completed,
            LoadingStep::FetchWorkItems => self.fetch_work_items = StepStatus::Completed,
            LoadingStep::FetchCommitInfo => self.fetch_commit_info = StepStatus::Completed,
            LoadingStep::AnalyzeDependencies => {
                self.analyze_dependencies = StepStatus::Completed;
            }
        }
        if self.current_step == Some(step) {
            self.current_step = None;
        }
    }

    /// Marks a step as skipped
    pub fn skip_step(&mut self, step: LoadingStep) {
        match step {
            LoadingStep::FetchPullRequests => self.fetch_pull_requests = StepStatus::Skipped,
            LoadingStep::FetchWorkItems => self.fetch_work_items = StepStatus::Skipped,
            LoadingStep::FetchCommitInfo => self.fetch_commit_info = StepStatus::Skipped,
            LoadingStep::AnalyzeDependencies => self.analyze_dependencies = StepStatus::Skipped,
        }
        if self.current_step == Some(step) {
            self.current_step = None;
        }
    }

    /// Updates progress counters for a step
    pub fn update_progress(&mut self, step: LoadingStep, fetched: usize, total: usize) {
        match step {
            LoadingStep::FetchWorkItems => {
                self.work_items_fetched = fetched;
                self.work_items_total = total;
            }
            LoadingStep::FetchCommitInfo => {
                self.commits_fetched = fetched;
                self.commits_total = total;
            }
            LoadingStep::AnalyzeDependencies => {
                self.prs_for_analysis = total;
            }
            _ => {}
        }
    }

    /// Returns the current step's progress message
    pub fn current_message(&self) -> String {
        match self.current_step {
            Some(LoadingStep::FetchPullRequests) => {
                LoadingStep::FetchPullRequests.progress_message(0, 0)
            }
            Some(LoadingStep::FetchWorkItems) => LoadingStep::FetchWorkItems
                .progress_message(self.work_items_fetched, self.work_items_total),
            Some(LoadingStep::FetchCommitInfo) => LoadingStep::FetchCommitInfo
                .progress_message(self.commits_fetched, self.commits_total),
            Some(LoadingStep::AnalyzeDependencies) => {
                LoadingStep::AnalyzeDependencies.progress_message(0, self.prs_for_analysis)
            }
            None => "Initializing...".to_string(),
        }
    }

    /// Returns whether dependency analysis is available
    /// Reserved for future use in dynamic step configuration
    #[allow(dead_code)]
    pub fn has_dependency_analysis(&self) -> bool {
        self.dependency_analysis_available
    }
}

// ============================================================================
// Step Data (accumulated results)
// ============================================================================

/// Intermediate data accumulated during loading steps
#[derive(Debug, Clone, Default)]
pub struct LoadingStepData {
    /// Total PRs fetched
    pub total_prs: usize,
    /// Work items fetch progress
    pub work_items_fetched: usize,
    pub work_items_total: usize,
    /// Commit info fetch progress
    pub commits_fetched: usize,
    /// Reserved for future progress tracking
    #[allow(dead_code)]
    pub commits_total: usize,
    /// Dependency graph result
    pub dependency_graph: Option<PRDependencyGraph>,
}

impl LoadingStepData {
    /// Merge a step result into this data, updating relevant fields
    pub fn merge_result(&mut self, result: &LoadingStepResult) {
        if let Some(count) = result.prs_fetched {
            self.total_prs = count;
            self.work_items_total = count;
        }
        if result.work_items_update.is_some() {
            self.work_items_fetched += 1;
        }
        if let Some(count) = result.commits_fetched {
            self.commits_fetched = count;
        }
    }
}

// ============================================================================
// Loading State Machine
// ============================================================================

/// Internal state of the loading wizard
#[derive(Debug)]
pub enum LoadingState {
    /// Initial state before starting
    Initializing,

    /// Background task is running, receiving progress updates via channel
    Running {
        progress: LoadingProgress,
        /// Accumulated step data from completed steps
        step_data: LoadingStepData,
    },

    /// All steps completed successfully
    Complete {
        /// Final step data with all accumulated results
        step_data: LoadingStepData,
    },

    /// An error occurred during loading
    Error {
        error: LoadingError,
        message: String,
        /// Progress at the time of error (to show which step failed)
        progress: Option<LoadingProgress>,
    },
}

// ============================================================================
// Loading Context (extracted from MergeApp for background task)
// ============================================================================

/// Context extracted from MergeApp for use in the background loading task.
///
/// This struct contains all the data needed to run the loading steps without
/// requiring mutable access to MergeApp. It's extracted once at the start
/// of the loading process.
#[derive(Clone)]
pub struct LoadingContext {
    /// API client for Azure DevOps operations
    pub client: crate::api::AzureDevOpsClient,
    /// Development branch to fetch PRs from
    pub dev_branch: String,
    /// Date filter for PRs (since date)
    pub since: Option<String>,
    /// Local repository path (for dependency analysis)
    pub local_repo: Option<String>,
    /// Network throttling: max concurrent network operations
    pub max_concurrent_network: usize,
    /// Network throttling: max concurrent processing operations
    pub max_concurrent_processing: usize,
}

impl LoadingContext {
    /// Extracts loading context from a MergeApp instance.
    pub fn from_app(app: &MergeApp) -> Self {
        Self {
            client: app.client().clone(),
            dev_branch: app.dev_branch().to_string(),
            since: app.since().map(String::from),
            local_repo: app.local_repo().map(String::from),
            max_concurrent_network: app.max_concurrent_network(),
            max_concurrent_processing: app.max_concurrent_processing(),
        }
    }

    /// Returns whether dependency analysis is available (local repo exists)
    pub fn has_local_repo(&self) -> bool {
        self.local_repo
            .as_ref()
            .is_some_and(|p| Path::new(p).exists())
    }
}

/// Channel receiver wrapper that allows Debug implementation
struct LoadingProgressReceiver(mpsc::Receiver<LoadingProgressMessage>);

impl std::fmt::Debug for LoadingProgressReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadingProgressReceiver")
            .finish_non_exhaustive()
    }
}

// ============================================================================
// Work Items Result (used in background task)
// ============================================================================

#[derive(Debug, Clone)]
pub struct WorkItemsResult {
    pub pr_index: usize,
    pub work_items: Vec<crate::models::WorkItem>,
}

/// The data loading state machine.
///
/// Manages the loading of pull requests, work items, commit info, and
/// dependency analysis using a channel-based wizard pattern.
pub struct DataLoadingState {
    /// Internal state machine
    state: LoadingState,
    /// Channel receiver for progress messages from background task
    receiver: Option<LoadingProgressReceiver>,
    /// Cached: whether local repo is available for dependency analysis
    has_local_repo: Option<bool>,
}

impl std::fmt::Debug for DataLoadingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataLoadingState")
            .field("state", &self.state)
            .field("has_local_repo", &self.has_local_repo)
            .finish_non_exhaustive()
    }
}

impl Default for DataLoadingState {
    fn default() -> Self {
        Self::new()
    }
}

impl DataLoadingState {
    pub fn new() -> Self {
        Self {
            state: LoadingState::Initializing,
            receiver: None,
            has_local_repo: None,
        }
    }

    // ========================================================================
    // State Management Methods
    // ========================================================================

    /// Get mutable reference to progress if in running state
    fn progress_mut(&mut self) -> Option<&mut LoadingProgress> {
        match &mut self.state {
            LoadingState::Running { progress, .. } => Some(progress),
            _ => None,
        }
    }

    /// Start a loading step
    fn start_step(&mut self, step: LoadingStep) {
        if let Some(progress) = self.progress_mut() {
            progress.start_step(step);
        }
    }

    /// Complete a loading step
    fn complete_step(&mut self, step: LoadingStep) {
        if let Some(progress) = self.progress_mut() {
            progress.complete_step(step);
        }
    }

    /// Update progress counters for a step
    fn update_step_progress(&mut self, step: LoadingStep, fetched: usize, total: usize) {
        if let Some(progress) = self.progress_mut() {
            progress.update_progress(step, fetched, total);
        }
    }

    /// Merge a step result into the accumulated step data
    fn merge_step_result(&mut self, result: &LoadingStepResult) {
        if let LoadingState::Running { step_data, .. } = &mut self.state {
            step_data.merge_result(result);
        }
    }

    /// Set error state with preserved progress
    fn set_error(&mut self, error: LoadingError) {
        // Preserve the current progress to show which step failed
        let current_progress = match &self.state {
            LoadingState::Running { progress, .. } => Some(progress.clone()),
            _ => None,
        };

        let message = error.message();
        self.state = LoadingState::Error {
            error,
            message,
            progress: current_progress,
        };
    }

    /// Start the background loading task
    fn start_background_task(&mut self, app: &MergeApp) {
        let ctx = LoadingContext::from_app(app);
        let has_local_repo = ctx.has_local_repo();
        self.has_local_repo = Some(has_local_repo);

        let (tx, rx) = mpsc::channel::<LoadingProgressMessage>(32);
        self.receiver = Some(LoadingProgressReceiver(rx));

        // Initialize the Running state
        self.state = LoadingState::Running {
            progress: LoadingProgress::new(has_local_repo),
            step_data: LoadingStepData::default(),
        };

        // Spawn the background task
        tokio::spawn(run_loading_task(ctx, tx));
    }

    /// Process a message received from the background task
    fn handle_progress_message(&mut self, msg: LoadingProgressMessage, app: &mut MergeApp) {
        match msg {
            LoadingProgressMessage::StepStarted(step) => {
                self.start_step(step);
            }
            LoadingProgressMessage::StepCompleted(step, result) => {
                self.complete_step(step);
                // Apply work items updates to app immediately
                if let Some(wi_result) = &result.work_items_update
                    && let Some(pr_with_wi) = app.pull_requests_mut().get_mut(wi_result.pr_index)
                {
                    pr_with_wi.work_items = wi_result.work_items.clone();
                }
                self.merge_step_result(&result);
            }
            LoadingProgressMessage::StepProgress(step, fetched, total) => {
                self.update_step_progress(step, fetched, total);
            }
            LoadingProgressMessage::AllComplete => {
                // Extract the accumulated data and transition to Complete state
                if let LoadingState::Running { step_data, .. } = &self.state {
                    self.state = LoadingState::Complete {
                        step_data: step_data.clone(),
                    };
                }
            }
            LoadingProgressMessage::Error(err) => {
                self.set_error(err);
            }
        }
    }

    /// Drain all pending messages from the channel
    fn drain_messages(&mut self) -> Vec<LoadingProgressMessage> {
        if let Some(LoadingProgressReceiver(ref mut rx)) = self.receiver {
            let mut msgs = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                msgs.push(msg);
            }
            msgs
        } else {
            Vec::new()
        }
    }

    /// Retry from the beginning after an error
    fn retry(&mut self, app: &MergeApp) {
        // Clear any existing receiver
        self.receiver = None;
        // Reset to initializing and start again
        self.state = LoadingState::Initializing;
        self.start_background_task(app);
    }

    /// Skip the current step (for optional steps like dependency analysis)
    fn skip_current_step(&mut self) {
        if let LoadingState::Error {
            progress: Some(prog),
            ..
        } = &self.state
            && prog.current_step == Some(LoadingStep::AnalyzeDependencies)
        {
            // Restore running state with skipped step
            let mut new_progress = prog.clone();
            new_progress.skip_step(LoadingStep::AnalyzeDependencies);
            self.state = LoadingState::Complete {
                step_data: LoadingStepData::default(),
            };
        }
    }
}

// ============================================================================
// Background Task Implementation
// ============================================================================

/// Runs the loading steps in a background task, sending progress updates via channel.
///
/// This function executes all loading steps sequentially, sending progress messages
/// to the UI through the provided channel. The UI can then update the display
/// as each step starts and completes.
async fn run_loading_task(ctx: LoadingContext, tx: mpsc::Sender<LoadingProgressMessage>) {
    // Helper macro to send a message or return if channel is closed
    macro_rules! send_or_return {
        ($tx:expr, $msg:expr) => {
            if $tx.send($msg).await.is_err() {
                return; // Channel closed, UI no longer listening
            }
        };
    }

    // Step 1: Fetch Pull Requests
    send_or_return!(
        tx,
        LoadingProgressMessage::StepStarted(LoadingStep::FetchPullRequests)
    );

    let prs = match fetch_pull_requests_impl(&ctx).await {
        Ok(prs) => prs,
        Err(e) => {
            let _ = tx.send(LoadingProgressMessage::Error(e)).await;
            return;
        }
    };

    let pr_count = prs.len();
    send_or_return!(
        tx,
        LoadingProgressMessage::StepCompleted(
            LoadingStep::FetchPullRequests,
            LoadingStepResult {
                prs_fetched: Some(pr_count),
                ..Default::default()
            }
        )
    );

    // Step 2: Fetch Work Items (parallel with progress updates)
    send_or_return!(
        tx,
        LoadingProgressMessage::StepStarted(LoadingStep::FetchWorkItems)
    );

    // Send initial progress
    send_or_return!(
        tx,
        LoadingProgressMessage::StepProgress(LoadingStep::FetchWorkItems, 0, pr_count)
    );

    match fetch_work_items_impl(&ctx, &prs, &tx).await {
        Ok(_) => {}
        Err(e) => {
            let _ = tx.send(LoadingProgressMessage::Error(e)).await;
            return;
        }
    }

    send_or_return!(
        tx,
        LoadingProgressMessage::StepCompleted(
            LoadingStep::FetchWorkItems,
            LoadingStepResult::default()
        )
    );

    // Step 3: Fetch Commit Info
    send_or_return!(
        tx,
        LoadingProgressMessage::StepStarted(LoadingStep::FetchCommitInfo)
    );

    // Count PRs needing commit info
    let commits_needed = prs
        .iter()
        .filter(|p| p.pr.last_merge_commit.is_none())
        .count();
    send_or_return!(
        tx,
        LoadingProgressMessage::StepProgress(LoadingStep::FetchCommitInfo, 0, commits_needed)
    );

    match fetch_commit_info_impl(&ctx, &prs, &tx).await {
        Ok(_) => {}
        Err(e) => {
            let _ = tx.send(LoadingProgressMessage::Error(e)).await;
            return;
        }
    }

    send_or_return!(
        tx,
        LoadingProgressMessage::StepCompleted(
            LoadingStep::FetchCommitInfo,
            LoadingStepResult {
                commits_fetched: Some(commits_needed),
                ..Default::default()
            }
        )
    );

    // Step 4: Analyze Dependencies (if local repo available)
    if ctx.has_local_repo() {
        send_or_return!(
            tx,
            LoadingProgressMessage::StepStarted(LoadingStep::AnalyzeDependencies)
        );

        send_or_return!(
            tx,
            LoadingProgressMessage::StepProgress(LoadingStep::AnalyzeDependencies, 0, pr_count)
        );

        match analyze_dependencies_impl(&ctx, &prs) {
            Ok(graph) => {
                send_or_return!(
                    tx,
                    LoadingProgressMessage::StepCompleted(
                        LoadingStep::AnalyzeDependencies,
                        LoadingStepResult {
                            dependencies_analyzed: Some(true),
                            ..Default::default()
                        }
                    )
                );
                // Note: The dependency graph will be set in the app when processing StepCompleted
                let _ = graph; // Graph is applied separately
            }
            Err(e) => {
                // Dependency analysis errors are non-fatal, just log and skip
                let _ = tx
                    .send(LoadingProgressMessage::StepCompleted(
                        LoadingStep::AnalyzeDependencies,
                        LoadingStepResult {
                            dependencies_analyzed: Some(false),
                            ..Default::default()
                        },
                    ))
                    .await;
                // Log the error but continue
                eprintln!("Dependency analysis failed (non-fatal): {:?}", e);
            }
        }
    }

    // All steps completed
    let _ = tx.send(LoadingProgressMessage::AllComplete).await;
}

/// Fetch pull requests from Azure DevOps API
async fn fetch_pull_requests_impl(
    ctx: &LoadingContext,
) -> Result<Vec<PullRequestWithWorkItems>, LoadingError> {
    let prs = ctx
        .client
        .fetch_pull_requests(&ctx.dev_branch, ctx.since.as_deref())
        .await
        .map_err(|e| LoadingError::ApiError(format!("Failed to fetch pull requests: {}", e)))?;

    let filtered_prs = api::filter_prs_without_merged_tag(prs);

    if filtered_prs.is_empty() {
        return Err(LoadingError::NoPullRequestsFound);
    }

    // Initialize PRs with empty work items
    Ok(filtered_prs
        .into_iter()
        .map(|pr| PullRequestWithWorkItems {
            pr,
            work_items: Vec::new(),
            selected: false,
        })
        .collect())
}

/// Fetch work items for all PRs in parallel with throttling
async fn fetch_work_items_impl(
    ctx: &LoadingContext,
    prs: &[PullRequestWithWorkItems],
    tx: &mpsc::Sender<LoadingProgressMessage>,
) -> Result<(), LoadingError> {
    use crate::utils::throttle::NetworkProcessor;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let network_processor = NetworkProcessor::new_with_limits(
        ctx.max_concurrent_network,
        ctx.max_concurrent_processing,
    );

    let total = prs.len();
    let completed = Arc::new(AtomicUsize::new(0));

    let mut tasks = Vec::new();

    for (index, pr_with_wi) in prs.iter().enumerate() {
        let client = ctx.client.clone();
        let pr_id = pr_with_wi.pr.id;
        let processor = network_processor.clone();
        let tx = tx.clone();
        let completed = completed.clone();

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
                Ok(work_items) => {
                    let count = completed.fetch_add(1, Ordering::SeqCst) + 1;

                    // Send progress update
                    let _ = tx
                        .send(LoadingProgressMessage::StepProgress(
                            LoadingStep::FetchWorkItems,
                            count,
                            total,
                        ))
                        .await;

                    // Send individual work item result
                    let _ = tx
                        .send(LoadingProgressMessage::StepCompleted(
                            LoadingStep::FetchWorkItems,
                            LoadingStepResult {
                                work_items_update: Some(WorkItemsResult {
                                    pr_index: index,
                                    work_items,
                                }),
                                ..Default::default()
                            },
                        ))
                        .await;

                    Ok(())
                }
                Err(e) => Err(e),
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete
    for task in tasks {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                return Err(LoadingError::ApiError(format!(
                    "Failed to fetch work items: {}",
                    e
                )));
            }
            Err(e) => {
                return Err(LoadingError::Other(format!("Task panicked: {}", e)));
            }
        }
    }

    Ok(())
}

/// Fetch commit information for PRs that don't have it
async fn fetch_commit_info_impl(
    ctx: &LoadingContext,
    prs: &[PullRequestWithWorkItems],
    tx: &mpsc::Sender<LoadingProgressMessage>,
) -> Result<(), LoadingError> {
    let mut fetched = 0;
    let total = prs
        .iter()
        .filter(|p| p.pr.last_merge_commit.is_none())
        .count();

    for pr_with_wi in prs {
        if pr_with_wi.pr.last_merge_commit.is_none() {
            match ctx.client.fetch_pr_commit(pr_with_wi.pr.id).await {
                Ok(_commit_info) => {
                    fetched += 1;
                    // Send progress update
                    let _ = tx
                        .send(LoadingProgressMessage::StepProgress(
                            LoadingStep::FetchCommitInfo,
                            fetched,
                            total,
                        ))
                        .await;
                }
                Err(e) => {
                    return Err(LoadingError::ApiError(format!(
                        "Failed to fetch commit for PR #{}: {}",
                        pr_with_wi.pr.id, e
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Analyze file dependencies using local repository
fn analyze_dependencies_impl(
    ctx: &LoadingContext,
    prs: &[PullRequestWithWorkItems],
) -> Result<Option<PRDependencyGraph>, LoadingError> {
    let local_repo = match &ctx.local_repo {
        Some(path) => path,
        None => return Ok(None),
    };

    let repo_path = Path::new(local_repo);
    if !repo_path.exists() {
        return Err(LoadingError::LocalRepoNotFound(local_repo.clone()));
    }

    // Build PRInfo list and sort by closed date (oldest first)
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
    let pr_dates: HashMap<i32, Option<String>> = prs
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
            (None, None) => a.id.cmp(&b.id),
        }
    });

    // Parallel fetch of file changes for each PR
    let pr_changes: HashMap<i32, Vec<FileChange>> = pr_infos
        .par_iter()
        .filter_map(|pr_info| {
            let commit_id = pr_info.commit_id.as_ref()?;
            match git::get_commit_changes_with_ranges(repo_path, commit_id) {
                Ok(changes) => Some((pr_info.id, changes)),
                Err(_) => Some((pr_info.id, Vec::new())),
            }
        })
        .collect();

    // Run parallel dependency analysis
    let analyzer = DependencyAnalyzer::new();
    let result = analyzer.analyze_parallel(&pr_infos, &pr_changes);

    Ok(Some(result.graph))
}

// ============================================================================
// UI Rendering Functions
// ============================================================================

/// Renders the loading step indicator showing all steps with their status
fn render_step_indicator(f: &mut Frame, area: Rect, progress: &LoadingProgress) {
    let steps = progress.steps();
    let total_steps = steps.len();

    // Build the step indicator spans
    let mut spans: Vec<Span> = Vec::new();

    for (i, (step, status)) in steps.iter().enumerate() {
        let step_num = i + 1;
        let step_name = step.display_name();

        // Choose style and symbol based on status
        let (symbol, style) = match status {
            StepStatus::Completed => (
                "✓",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            StepStatus::InProgress => (
                "●",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            StepStatus::Pending => ("○", Style::default().fg(Color::DarkGray)),
            StepStatus::Skipped => ("−", Style::default().fg(Color::DarkGray)),
        };

        // Number style matches the status
        let num_style = match status {
            StepStatus::Completed => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            StepStatus::InProgress => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            _ => Style::default().fg(Color::DarkGray),
        };

        // Step name style
        let name_style = match status {
            StepStatus::Completed => Style::default().fg(Color::Green),
            StepStatus::InProgress => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            _ => Style::default().fg(Color::DarkGray),
        };

        // Add step: "1 ✓ Fetch PRs"
        spans.push(Span::styled(format!("{}", step_num), num_style));
        spans.push(Span::styled(format!(" {} ", symbol), style));
        spans.push(Span::styled(step_name.to_string(), name_style));

        // Add connector between steps (except last)
        if i < total_steps - 1 {
            spans.push(Span::styled("  →  ", Style::default().fg(Color::DarkGray)));
        }
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).alignment(Alignment::Center);
    f.render_widget(paragraph, area);
}

/// Renders the current step progress details
fn render_current_step_progress(f: &mut Frame, area: Rect, progress: &LoadingProgress) {
    let message = progress.current_message();

    // Build content with current step message
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            message,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Please wait...",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Current Step")
            .title_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(paragraph, area);
}

/// Helper to style hotkeys in error messages
fn style_hotkey_line(line: &str, key_style: Style) -> Line<'_> {
    let mut spans = Vec::new();
    let mut remaining = line;

    while let Some(start) = remaining.find('\'') {
        // Add text before the quote
        if start > 0 {
            spans.push(Span::styled(
                &remaining[..start],
                Style::default().fg(Color::Gray),
            ));
        }

        // Find closing quote
        if let Some(end) = remaining[start + 1..].find('\'') {
            let key = &remaining[start + 1..start + 1 + end];
            spans.push(Span::styled(format!("'{}'", key), key_style));
            remaining = &remaining[start + 1 + end + 1..];
        } else {
            spans.push(Span::styled(
                &remaining[start..],
                Style::default().fg(Color::Gray),
            ));
            remaining = "";
            break;
        }
    }

    if !remaining.is_empty() {
        spans.push(Span::styled(remaining, Style::default().fg(Color::Gray)));
    }

    Line::from(spans)
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for DataLoadingState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Length(3), // Step indicator
                Constraint::Min(5),    // Current step progress / Error
            ])
            .split(f.area());

        // Title - color changes based on state
        let (title_text, title_color) = match &self.state {
            LoadingState::Error { .. } => ("Loading Data - Error", Color::Red),
            _ => ("Loading Data", Color::Green),
        };
        let title = Paragraph::new(title_text)
            .style(
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        match &self.state {
            LoadingState::Initializing => {
                // Show default step indicator for initial state
                let has_local_repo = self
                    .has_local_repo
                    .unwrap_or_else(|| app.local_repo().is_some());
                let progress = LoadingProgress::new(has_local_repo);

                // Step indicator
                let step_block = Block::default()
                    .borders(Borders::ALL)
                    .title("Steps")
                    .title_style(Style::default().fg(Color::Cyan));
                let inner_area = step_block.inner(chunks[1]);
                f.render_widget(step_block, chunks[1]);
                render_step_indicator(f, inner_area, &progress);

                // Progress area
                let status = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "Initializing...",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Please wait...",
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Current Step")
                        .title_style(Style::default().fg(Color::Cyan)),
                );
                f.render_widget(status, chunks[2]);
            }
            LoadingState::Running { progress, .. } => {
                // Step indicator
                let step_block = Block::default()
                    .borders(Borders::ALL)
                    .title("Steps")
                    .title_style(Style::default().fg(Color::Cyan));
                let inner_area = step_block.inner(chunks[1]);
                f.render_widget(step_block, chunks[1]);
                render_step_indicator(f, inner_area, progress);

                // Current step progress
                render_current_step_progress(f, chunks[2], progress);
            }
            LoadingState::Complete { .. } => {
                // This state is transient - we transition to PR selection immediately
                let has_local_repo = self
                    .has_local_repo
                    .unwrap_or_else(|| app.local_repo().is_some());
                let mut progress = LoadingProgress::new(has_local_repo);
                // Mark all steps as completed
                progress.complete_step(LoadingStep::FetchPullRequests);
                progress.complete_step(LoadingStep::FetchWorkItems);
                progress.complete_step(LoadingStep::FetchCommitInfo);
                if has_local_repo {
                    progress.complete_step(LoadingStep::AnalyzeDependencies);
                }

                let step_block = Block::default()
                    .borders(Borders::ALL)
                    .title("Steps")
                    .title_style(Style::default().fg(Color::Green));
                let inner_area = step_block.inner(chunks[1]);
                f.render_widget(step_block, chunks[1]);
                render_step_indicator(f, inner_area, &progress);

                let status = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "Loading complete!",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )),
                ])
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Current Step")
                        .title_style(Style::default().fg(Color::Green)),
                );
                f.render_widget(status, chunks[2]);
            }
            LoadingState::Error {
                message, progress, ..
            } => {
                // Show step indicator with error state - use preserved progress if available
                let display_progress = match progress {
                    Some(p) => p.clone(),
                    None => {
                        let has_local_repo = self
                            .has_local_repo
                            .unwrap_or_else(|| app.local_repo().is_some());
                        LoadingProgress::new(has_local_repo)
                    }
                };

                // Step indicator
                let step_block = Block::default()
                    .borders(Borders::ALL)
                    .title("Steps")
                    .title_style(Style::default().fg(Color::Red));
                let inner_area = step_block.inner(chunks[1]);
                f.render_widget(step_block, chunks[1]);
                render_step_indicator(f, inner_area, &display_progress);

                // Error message with styled hotkeys
                let key_style = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);

                let message_lines: Vec<Line> = message
                    .lines()
                    .map(|line| {
                        if line.starts_with("Options:") {
                            Line::from(vec![Span::styled(line, Style::default().fg(Color::Cyan))])
                        } else if line.starts_with("  •") || line.starts_with("   •") {
                            style_hotkey_line(line, key_style)
                        } else {
                            Line::from(line)
                        }
                    })
                    .collect();

                let error_paragraph = Paragraph::new(message_lines)
                    .style(Style::default().fg(Color::White))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Error")
                            .title_style(Style::default().fg(Color::Red)),
                    )
                    .wrap(Wrap { trim: true })
                    .alignment(Alignment::Left);

                f.render_widget(error_paragraph, chunks[2]);
            }
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> StateChange<MergeState> {
        // First, collect any pending messages from the background task
        let messages = self.drain_messages();
        for msg in messages {
            self.handle_progress_message(msg, app);
        }

        // Handle state-specific key presses and transitions
        match &self.state {
            LoadingState::Initializing => {
                // Start background task on first tick (Null key)
                if code == KeyCode::Null {
                    self.start_background_task(app);
                    return StateChange::Keep;
                }
            }
            LoadingState::Running { .. } => {
                // Only 'q' to quit during loading
                if code == KeyCode::Char('q') {
                    return StateChange::Exit;
                }
            }
            LoadingState::Complete { step_data } => {
                // Apply dependency graph if available
                if let Some(graph) = &step_data.dependency_graph {
                    app.set_dependency_graph(graph.clone());
                }
                // Automatically transition to PR selection
                return StateChange::Change(MergeState::PullRequestSelection(
                    PullRequestSelectionState::new(),
                ));
            }
            LoadingState::Error { error, .. } => match code {
                KeyCode::Char('r') if error.can_retry() => {
                    self.retry(app);
                    return StateChange::Keep;
                }
                KeyCode::Char('s') if error.can_skip() => {
                    self.skip_current_step();
                    return StateChange::Keep;
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    return StateChange::Exit;
                }
                _ => {}
            },
        }

        // Default: allow 'q' to quit at any time
        if code == KeyCode::Char('q') {
            return StateChange::Exit;
        }

        StateChange::Keep
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

    // ========================================================================
    // Helper functions for creating test states
    // ========================================================================

    fn create_running_state(
        has_local_repo: bool,
        current_step: Option<LoadingStep>,
    ) -> DataLoadingState {
        let mut progress = LoadingProgress::new(has_local_repo);
        if let Some(step) = current_step {
            progress.start_step(step);
        }
        DataLoadingState {
            state: LoadingState::Running {
                progress,
                step_data: LoadingStepData::default(),
            },
            receiver: None,
            has_local_repo: Some(has_local_repo),
        }
    }

    fn create_running_state_with_progress(
        has_local_repo: bool,
        completed_steps: &[LoadingStep],
        current_step: Option<LoadingStep>,
        work_items_progress: Option<(usize, usize)>,
        commits_progress: Option<(usize, usize)>,
    ) -> DataLoadingState {
        let mut progress = LoadingProgress::new(has_local_repo);
        for step in completed_steps {
            progress.start_step(*step);
            progress.complete_step(*step);
        }
        if let Some(step) = current_step {
            progress.start_step(step);
        }
        if let Some((fetched, total)) = work_items_progress {
            progress.update_progress(LoadingStep::FetchWorkItems, fetched, total);
        }
        if let Some((fetched, total)) = commits_progress {
            progress.update_progress(LoadingStep::FetchCommitInfo, fetched, total);
        }
        DataLoadingState {
            state: LoadingState::Running {
                progress,
                step_data: LoadingStepData::default(),
            },
            receiver: None,
            has_local_repo: Some(has_local_repo),
        }
    }

    fn create_error_state(
        has_local_repo: bool,
        error: LoadingError,
        completed_steps: &[LoadingStep],
        current_step: Option<LoadingStep>,
    ) -> DataLoadingState {
        let mut progress = LoadingProgress::new(has_local_repo);
        for step in completed_steps {
            progress.start_step(*step);
            progress.complete_step(*step);
        }
        if let Some(step) = current_step {
            progress.start_step(step);
        }
        let message = error.message();
        DataLoadingState {
            state: LoadingState::Error {
                error,
                message,
                progress: Some(progress),
            },
            receiver: None,
            has_local_repo: Some(has_local_repo),
        }
    }

    fn create_complete_state(has_local_repo: bool) -> DataLoadingState {
        DataLoadingState {
            state: LoadingState::Complete {
                step_data: LoadingStepData::default(),
            },
            receiver: None,
            has_local_repo: Some(has_local_repo),
        }
    }

    // ========================================================================
    // Initializing State Tests
    // ========================================================================

    /// # Data Loading State - Initializing
    ///
    /// Tests the initial loading state before background task starts.
    ///
    /// ## Test Scenario
    /// - Creates a new data loading state
    /// - Renders the state before any loading operations start
    ///
    /// ## Expected Outcome
    /// - Should display "Initializing..." message
    /// - Should show step indicator with all steps pending
    #[test]
    fn test_loading_initializing() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = DataLoadingState::new();
            harness.render_state(&mut state);

            assert_snapshot!("initializing", harness.backend());
        });
    }

    // ========================================================================
    // Running State Tests - Step Progress
    // ========================================================================

    /// # Data Loading State - Fetch PRs Started
    ///
    /// Tests the display when fetch pull requests step starts.
    ///
    /// ## Expected Outcome
    /// - Step 1 should show as in-progress (yellow)
    /// - Steps 2-4 should be pending (gray)
    #[test]
    fn test_loading_step_fetch_prs_started() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_running_state(true, Some(LoadingStep::FetchPullRequests));
            harness.render_state(&mut state);

            assert_snapshot!("step_fetch_prs_started", harness.backend());
        });
    }

    /// # Data Loading State - Fetch PRs Completed
    ///
    /// Tests the display when fetch pull requests step completes.
    ///
    /// ## Expected Outcome
    /// - Step 1 should show as completed (green checkmark)
    /// - Steps 2-4 should be pending (gray)
    #[test]
    fn test_loading_step_fetch_prs_completed() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_running_state_with_progress(
                true,
                &[LoadingStep::FetchPullRequests],
                None,
                None,
                None,
            );
            harness.render_state(&mut state);

            assert_snapshot!("step_fetch_prs_completed", harness.backend());
        });
    }

    /// # Data Loading State - Fetch Work Items Started
    ///
    /// Tests the display when fetch work items step starts.
    #[test]
    fn test_loading_step_fetch_work_items_started() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_running_state_with_progress(
                true,
                &[LoadingStep::FetchPullRequests],
                Some(LoadingStep::FetchWorkItems),
                Some((0, 10)),
                None,
            );
            harness.render_state(&mut state);

            assert_snapshot!("step_fetch_work_items_started", harness.backend());
        });
    }

    /// # Data Loading State - Fetch Work Items Progress 50%
    ///
    /// Tests the display with work items fetch at 50% progress.
    #[test]
    fn test_loading_step_fetch_work_items_progress_50() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_running_state_with_progress(
                true,
                &[LoadingStep::FetchPullRequests],
                Some(LoadingStep::FetchWorkItems),
                Some((5, 10)),
                None,
            );
            harness.render_state(&mut state);

            assert_snapshot!("step_fetch_work_items_progress_50", harness.backend());
        });
    }

    /// # Data Loading State - Fetch Commit Info Started
    ///
    /// Tests the display when fetch commit info step starts.
    #[test]
    fn test_loading_step_fetch_commits_started() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_running_state_with_progress(
                true,
                &[LoadingStep::FetchPullRequests, LoadingStep::FetchWorkItems],
                Some(LoadingStep::FetchCommitInfo),
                None,
                Some((0, 5)),
            );
            harness.render_state(&mut state);

            assert_snapshot!("step_fetch_commits_started", harness.backend());
        });
    }

    /// # Data Loading State - Fetch Commit Info Progress
    ///
    /// Tests the display with commit info fetch at partial progress.
    #[test]
    fn test_loading_step_fetch_commits_progress() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_running_state_with_progress(
                true,
                &[LoadingStep::FetchPullRequests, LoadingStep::FetchWorkItems],
                Some(LoadingStep::FetchCommitInfo),
                None,
                Some((2, 5)),
            );
            harness.render_state(&mut state);

            assert_snapshot!("step_fetch_commits_progress", harness.backend());
        });
    }

    /// # Data Loading State - Analyze Dependencies Started
    ///
    /// Tests the display when dependency analysis step starts.
    #[test]
    fn test_loading_step_analyze_deps_started() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_running_state_with_progress(
                true,
                &[
                    LoadingStep::FetchPullRequests,
                    LoadingStep::FetchWorkItems,
                    LoadingStep::FetchCommitInfo,
                ],
                Some(LoadingStep::AnalyzeDependencies),
                None,
                None,
            );
            harness.render_state(&mut state);

            assert_snapshot!("step_analyze_deps_started", harness.backend());
        });
    }

    /// # Data Loading State - Dependencies Skipped (No Local Repo)
    ///
    /// Tests that dependency analysis shows as skipped when no local repo.
    #[test]
    fn test_loading_step_analyze_deps_skipped() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            // Without local repo, dependencies step is skipped
            let mut state = create_running_state_with_progress(
                false,
                &[
                    LoadingStep::FetchPullRequests,
                    LoadingStep::FetchWorkItems,
                    LoadingStep::FetchCommitInfo,
                ],
                None,
                None,
                None,
            );
            harness.render_state(&mut state);

            assert_snapshot!("step_analyze_deps_skipped", harness.backend());
        });
    }

    // ========================================================================
    // Complete State Tests
    // ========================================================================

    /// # Data Loading State - Complete All Steps
    ///
    /// Tests the display when all steps are completed.
    #[test]
    fn test_loading_complete_all_steps() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_complete_state(true);
            harness.render_state(&mut state);

            assert_snapshot!("complete_all_steps", harness.backend());
        });
    }

    /// # Data Loading State - Complete with Dependencies Skipped
    ///
    /// Tests the display when loading completes without dependency analysis.
    #[test]
    fn test_loading_complete_deps_skipped() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_complete_state(false);
            harness.render_state(&mut state);

            assert_snapshot!("complete_deps_skipped", harness.backend());
        });
    }

    // ========================================================================
    // Error State Tests
    // ========================================================================

    /// # Data Loading State - Error No PRs Found
    ///
    /// Tests the display when no pull requests are found.
    #[test]
    fn test_loading_error_no_prs_found() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_error_state(
                true,
                LoadingError::NoPullRequestsFound,
                &[],
                Some(LoadingStep::FetchPullRequests),
            );
            harness.render_state(&mut state);

            assert_snapshot!("error_no_prs_found", harness.backend());
        });
    }

    /// # Data Loading State - Error API Error on Step 1
    ///
    /// Tests the display when API error occurs during PR fetch.
    #[test]
    fn test_loading_error_api_at_step1() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_error_state(
                true,
                LoadingError::ApiError("Connection refused".to_string()),
                &[],
                Some(LoadingStep::FetchPullRequests),
            );
            harness.render_state(&mut state);

            assert_snapshot!("error_api_at_step1", harness.backend());
        });
    }

    /// # Data Loading State - Error API Error on Step 2
    ///
    /// Tests the display when API error occurs during work items fetch.
    #[test]
    fn test_loading_error_api_at_step2() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_error_state(
                true,
                LoadingError::ApiError("Timeout fetching work items".to_string()),
                &[LoadingStep::FetchPullRequests],
                Some(LoadingStep::FetchWorkItems),
            );
            harness.render_state(&mut state);

            assert_snapshot!("error_api_at_step2", harness.backend());
        });
    }

    /// # Data Loading State - Error Local Repo Not Found (Skippable)
    ///
    /// Tests the display when local repo is not found (skippable error).
    #[test]
    fn test_loading_error_local_repo_not_found() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = create_error_state(
                true,
                LoadingError::LocalRepoNotFound("/path/to/repo".to_string()),
                &[
                    LoadingStep::FetchPullRequests,
                    LoadingStep::FetchWorkItems,
                    LoadingStep::FetchCommitInfo,
                ],
                Some(LoadingStep::AnalyzeDependencies),
            );
            harness.render_state(&mut state);

            assert_snapshot!("error_local_repo_not_found", harness.backend());
        });
    }

    // ========================================================================
    // Key Handling Tests
    // ========================================================================

    /// # Data Loading State - Quit During Loading
    ///
    /// Tests that pressing 'q' exits during loading.
    #[tokio::test]
    async fn test_data_loading_quit() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = DataLoadingState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # Data Loading State - First Null Key Starts Background Task
    ///
    /// Tests that first Null key starts the background task.
    #[tokio::test]
    async fn test_data_loading_first_tick_starts_task() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = DataLoadingState::new();
        assert!(matches!(state.state, LoadingState::Initializing));

        let result =
            ModeState::process_key(&mut state, KeyCode::Null, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        // After first tick, should transition to Running state
        assert!(matches!(state.state, LoadingState::Running { .. }));
    }

    /// # Data Loading State - Other Keys Ignored During Loading
    ///
    /// Tests that other keys are ignored during loading.
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

    /// # Data Loading State - Complete State Transitions to PR Selection
    ///
    /// Tests that Complete state automatically transitions to PR selection.
    #[tokio::test]
    async fn test_data_loading_complete_transitions() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = create_complete_state(true);

        let result =
            ModeState::process_key(&mut state, KeyCode::Null, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Data Loading State - Error State Retry
    ///
    /// Tests that 'r' key retries on retryable error.
    #[tokio::test]
    async fn test_data_loading_error_retry() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = create_error_state(
            true,
            LoadingError::ApiError("Test error".to_string()),
            &[],
            Some(LoadingStep::FetchPullRequests),
        );

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('r'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        // After retry, should be in Running state again
        assert!(matches!(state.state, LoadingState::Running { .. }));
    }

    /// # Data Loading State - Error State Escape
    ///
    /// Tests that Esc key exits on error.
    #[tokio::test]
    async fn test_data_loading_error_escape() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = create_error_state(
            true,
            LoadingError::NoPullRequestsFound,
            &[],
            Some(LoadingStep::FetchPullRequests),
        );

        let result =
            ModeState::process_key(&mut state, KeyCode::Esc, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Exit));
    }

    // ========================================================================
    // Unit Tests for Types
    // ========================================================================

    /// # Data Loading State - Default Trait Implementation
    ///
    /// Tests the Default trait implementation.
    #[test]
    fn test_data_loading_default() {
        let state = DataLoadingState::default();
        assert!(matches!(state.state, LoadingState::Initializing));
        assert!(state.receiver.is_none());
        assert!(state.has_local_repo.is_none());
    }

    /// # LoadingStep - Display Names
    ///
    /// Tests that all loading steps have correct display names.
    #[test]
    fn test_loading_step_display_names() {
        assert_eq!(LoadingStep::FetchPullRequests.display_name(), "Fetch PRs");
        assert_eq!(LoadingStep::FetchWorkItems.display_name(), "Work Items");
        assert_eq!(LoadingStep::FetchCommitInfo.display_name(), "Commit Info");
        assert_eq!(
            LoadingStep::AnalyzeDependencies.display_name(),
            "Dependencies"
        );
    }

    /// # LoadingStep - Progress Messages
    ///
    /// Tests that loading steps generate correct progress messages.
    #[test]
    fn test_loading_step_progress_messages() {
        assert_eq!(
            LoadingStep::FetchPullRequests.progress_message(0, 0),
            "Fetching pull requests..."
        );
        assert_eq!(
            LoadingStep::FetchWorkItems.progress_message(5, 10),
            "Fetching work items (5/10)..."
        );
        assert_eq!(
            LoadingStep::FetchWorkItems.progress_message(0, 0),
            "Fetching work items..."
        );
        assert_eq!(
            LoadingStep::FetchCommitInfo.progress_message(2, 5),
            "Fetching commit info (2/5)..."
        );
        assert_eq!(
            LoadingStep::AnalyzeDependencies.progress_message(0, 15),
            "Analyzing dependencies (15 PRs)..."
        );
    }

    /// # LoadingError - Can Retry
    ///
    /// Tests that error types correctly report retry capability.
    #[test]
    fn test_loading_error_can_retry() {
        assert!(!LoadingError::NoPullRequestsFound.can_retry());
        assert!(LoadingError::ApiError("test".to_string()).can_retry());
        assert!(LoadingError::NetworkTimeout("test".to_string()).can_retry());
        assert!(!LoadingError::LocalRepoNotFound("test".to_string()).can_retry());
        assert!(LoadingError::Other("test".to_string()).can_retry());
    }

    /// # LoadingError - Can Skip
    ///
    /// Tests that error types correctly report skip capability.
    #[test]
    fn test_loading_error_can_skip() {
        assert!(!LoadingError::NoPullRequestsFound.can_skip());
        assert!(!LoadingError::ApiError("test".to_string()).can_skip());
        assert!(!LoadingError::NetworkTimeout("test".to_string()).can_skip());
        assert!(LoadingError::LocalRepoNotFound("test".to_string()).can_skip());
        assert!(!LoadingError::Other("test".to_string()).can_skip());
    }

    /// # LoadingProgress - Steps List
    ///
    /// Tests that progress correctly reports step list.
    #[test]
    fn test_loading_progress_steps() {
        let progress = LoadingProgress::new(true);
        let steps = progress.steps();
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].0, LoadingStep::FetchPullRequests);
        assert_eq!(steps[1].0, LoadingStep::FetchWorkItems);
        assert_eq!(steps[2].0, LoadingStep::FetchCommitInfo);
        assert_eq!(steps[3].0, LoadingStep::AnalyzeDependencies);

        // Without local repo, dependencies should be skipped
        let progress_no_repo = LoadingProgress::new(false);
        let steps_no_repo = progress_no_repo.steps();
        assert_eq!(steps_no_repo[3].1, StepStatus::Skipped);
    }

    /// # LoadingProgress - Step Status Updates
    ///
    /// Tests that progress correctly updates step status.
    #[test]
    fn test_loading_progress_status_updates() {
        let mut progress = LoadingProgress::new(true);

        // Initially all pending
        assert_eq!(progress.steps()[0].1, StepStatus::Pending);

        // Start step
        progress.start_step(LoadingStep::FetchPullRequests);
        assert_eq!(progress.steps()[0].1, StepStatus::InProgress);

        // Complete step
        progress.complete_step(LoadingStep::FetchPullRequests);
        assert_eq!(progress.steps()[0].1, StepStatus::Completed);
    }
}
