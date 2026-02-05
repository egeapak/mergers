// Allow deprecated RepositorySetupError usage until full migration to GitError
#![allow(deprecated)]

use super::MergeState;
use crate::{
    api::AzureDevOpsClient,
    core::state::{MergePhase, StateCreateConfig, StateManager},
    git,
    models::CherryPickItem,
    ui::apps::MergeApp,
    ui::state::typed::{ModeState, StateChange},
    ui::state::{CherryPickState, ErrorState},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

// ============================================================================
// Channel-Based Message Types
// ============================================================================

/// Messages sent from the background setup task to the UI.
#[derive(Debug, Clone)]
pub enum ProgressMessage {
    /// A step has started executing
    StepStarted(WizardStep),
    /// A step completed successfully with optional result data
    StepCompleted(WizardStep, StepResult),
    /// All steps completed successfully
    AllComplete,
    /// An error occurred during setup
    Error(SetupError),
}

/// Result data from completing a wizard step.
#[derive(Debug, Clone, Default)]
pub struct StepResult {
    /// SSH URL fetched from Azure DevOps (FetchDetails step)
    pub ssh_url: Option<String>,
    /// Repository path after clone/worktree creation
    pub repo_path: Option<PathBuf>,
    /// Whether this is a worktree (vs clone)
    pub is_worktree: bool,
    /// Base repo path for worktree cleanup
    pub base_repo_path: Option<PathBuf>,
    /// Branch name created
    pub branch_name: Option<String>,
    /// Cherry-pick items prepared
    pub cherry_pick_items: Option<Vec<CherryPickItem>>,
}

/// Error types that can occur during setup.
#[derive(Debug, Clone)]
pub enum SetupError {
    /// A branch already exists
    BranchExists(String),
    /// A worktree already exists at the given path
    WorktreeExists(String),
    /// A generic error with message
    Other(String),
}

impl From<git::RepositorySetupError> for SetupError {
    fn from(err: git::RepositorySetupError) -> Self {
        match err {
            git::RepositorySetupError::BranchExists(b) => SetupError::BranchExists(b),
            git::RepositorySetupError::WorktreeExists(p) => SetupError::WorktreeExists(p),
            git::RepositorySetupError::Other(m) => SetupError::Other(m),
        }
    }
}

impl From<SetupError> for git::RepositorySetupError {
    fn from(err: SetupError) -> Self {
        match err {
            SetupError::BranchExists(b) => git::RepositorySetupError::BranchExists(b),
            SetupError::WorktreeExists(p) => git::RepositorySetupError::WorktreeExists(p),
            SetupError::Other(m) => git::RepositorySetupError::Other(m),
        }
    }
}

// ============================================================================
// Setup Context (extracted from MergeApp for background task)
// ============================================================================

/// Context extracted from MergeApp for use in the background setup task.
///
/// This struct contains all the data needed to run the setup steps without
/// requiring mutable access to MergeApp. It's extracted once at the start
/// of the setup process.
pub struct SetupContext {
    /// API client for Azure DevOps operations
    pub client: AzureDevOpsClient,
    /// Whether we're in clone mode (no local_repo) or worktree mode
    pub is_clone_mode: bool,
    /// Local repository path (worktree mode only)
    pub local_repo: Option<String>,
    /// Target branch name
    pub target_branch: String,
    /// Version string for branch naming
    pub version: String,
    /// Whether to run git hooks
    pub run_hooks: bool,
    /// Selected PRs with their merge commits for cherry-picking
    pub selected_prs: Vec<SelectedPrInfo>,
    /// State manager for creating state files from background task
    pub state_manager: Arc<Mutex<StateManager>>,
    /// Configuration for state file creation
    pub state_config: StateCreateConfig,
}

/// Minimal PR info needed for cherry-pick preparation.
#[derive(Clone, Debug)]
pub struct SelectedPrInfo {
    pub pr_id: i32,
    pub pr_title: String,
    pub merge_commit_id: Option<String>,
}

impl SetupContext {
    /// Extracts setup context from a MergeApp instance.
    pub fn from_app(app: &MergeApp) -> Option<Self> {
        let version = app.version()?.to_string();
        let selected_prs = app
            .get_selected_prs()
            .iter()
            .map(|pr| SelectedPrInfo {
                pr_id: pr.pr.id,
                pr_title: pr.pr.title.clone(),
                merge_commit_id: pr
                    .pr
                    .last_merge_commit
                    .as_ref()
                    .map(|c| c.commit_id.clone()),
            })
            .collect();

        Some(Self {
            client: app.client().clone(),
            is_clone_mode: app.local_repo().is_none(),
            local_repo: app.local_repo().map(String::from),
            target_branch: app.target_branch().to_string(),
            version,
            run_hooks: app.run_hooks(),
            selected_prs,
            state_manager: app.state_manager(),
            state_config: app.state_create_config(),
        })
    }
}

// ============================================================================
// Wizard Step Definitions
// ============================================================================

/// Represents the individual steps in the repository setup wizard.
/// Steps are split into granular sub-steps for better visual progress feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    /// Fetch repository SSH URL from Azure DevOps (clone mode only)
    FetchDetails,
    /// Check for existing worktrees/branches that might conflict
    CheckPrerequisites,
    /// Fetch target branch from remote (worktree mode only)
    FetchTargetBranch,
    /// Clone repository or create worktree
    CloneOrWorktree,
    /// Configure repository settings (hooks, etc.)
    ConfigureRepository,
    /// Create the patch branch for cherry-picking
    CreateBranch,
    /// Prepare the list of commits to cherry-pick
    PrepareCherryPicks,
    /// Create state file for cross-mode resume support
    InitializeState,
}

impl WizardStep {
    /// Returns the display name for this step
    fn display_name(&self, is_clone_mode: bool) -> &'static str {
        match self {
            WizardStep::FetchDetails => "Fetch Details",
            WizardStep::CheckPrerequisites => "Check Prerequisites",
            WizardStep::FetchTargetBranch => "Fetch Branch",
            WizardStep::CloneOrWorktree => {
                if is_clone_mode {
                    "Clone Repository"
                } else {
                    "Create Worktree"
                }
            }
            WizardStep::ConfigureRepository => "Configure",
            WizardStep::CreateBranch => "Create Branch",
            WizardStep::PrepareCherryPicks => "Prepare Items",
            WizardStep::InitializeState => "Initialize",
        }
    }

    /// Returns the progress message for this step
    fn progress_message(&self, is_clone_mode: bool) -> &'static str {
        match self {
            WizardStep::FetchDetails => "Fetching repository details...",
            WizardStep::CheckPrerequisites => "Checking prerequisites...",
            WizardStep::FetchTargetBranch => "Fetching target branch...",
            WizardStep::CloneOrWorktree => {
                if is_clone_mode {
                    "Cloning repository..."
                } else {
                    "Creating worktree..."
                }
            }
            WizardStep::ConfigureRepository => "Configuring repository...",
            WizardStep::CreateBranch => "Creating patch branch...",
            WizardStep::PrepareCherryPicks => "Preparing cherry-pick items...",
            WizardStep::InitializeState => "Initializing state file...",
        }
    }
}

/// Status of a wizard step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
}

/// Tracks progress through the setup wizard steps
#[derive(Debug, Clone)]
pub struct WizardProgress {
    /// Whether we're in clone mode (vs worktree mode)
    is_clone_mode: bool,
    /// Status of fetch details step (clone mode only)
    fetch_details: StepStatus,
    /// Status of check prerequisites step
    check_prerequisites: StepStatus,
    /// Status of fetch target branch step (worktree mode only)
    fetch_target_branch: StepStatus,
    /// Status of clone/worktree step
    clone_or_worktree: StepStatus,
    /// Status of configure repository step
    configure_repository: StepStatus,
    /// Status of create branch step
    create_branch: StepStatus,
    /// Status of prepare cherry-picks step
    prepare_cherry_picks: StepStatus,
    /// Status of initialize state step
    initialize_state: StepStatus,
    /// Current step being executed
    current_step: Option<WizardStep>,
}

impl WizardProgress {
    /// Creates a new wizard progress tracker
    pub fn new(is_clone_mode: bool) -> Self {
        Self {
            is_clone_mode,
            fetch_details: if is_clone_mode {
                StepStatus::Pending
            } else {
                StepStatus::Skipped
            },
            check_prerequisites: StepStatus::Pending,
            fetch_target_branch: if is_clone_mode {
                StepStatus::Skipped
            } else {
                StepStatus::Pending
            },
            clone_or_worktree: StepStatus::Pending,
            configure_repository: StepStatus::Pending,
            create_branch: StepStatus::Pending,
            prepare_cherry_picks: StepStatus::Pending,
            initialize_state: StepStatus::Pending,
            current_step: None,
        }
    }

    /// Returns the list of steps with their status (excluding skipped steps)
    pub fn steps(&self) -> Vec<(WizardStep, StepStatus)> {
        let mut steps = Vec::new();
        if self.is_clone_mode {
            steps.push((WizardStep::FetchDetails, self.fetch_details));
        }
        steps.push((WizardStep::CheckPrerequisites, self.check_prerequisites));
        if !self.is_clone_mode {
            steps.push((WizardStep::FetchTargetBranch, self.fetch_target_branch));
        }
        steps.push((WizardStep::CloneOrWorktree, self.clone_or_worktree));
        steps.push((WizardStep::ConfigureRepository, self.configure_repository));
        steps.push((WizardStep::CreateBranch, self.create_branch));
        steps.push((WizardStep::PrepareCherryPicks, self.prepare_cherry_picks));
        steps.push((WizardStep::InitializeState, self.initialize_state));
        steps
    }

    /// Sets a step to in-progress status
    pub fn start_step(&mut self, step: WizardStep) {
        self.current_step = Some(step);
        match step {
            WizardStep::FetchDetails => self.fetch_details = StepStatus::InProgress,
            WizardStep::CheckPrerequisites => self.check_prerequisites = StepStatus::InProgress,
            WizardStep::FetchTargetBranch => self.fetch_target_branch = StepStatus::InProgress,
            WizardStep::CloneOrWorktree => self.clone_or_worktree = StepStatus::InProgress,
            WizardStep::ConfigureRepository => self.configure_repository = StepStatus::InProgress,
            WizardStep::CreateBranch => self.create_branch = StepStatus::InProgress,
            WizardStep::PrepareCherryPicks => self.prepare_cherry_picks = StepStatus::InProgress,
            WizardStep::InitializeState => self.initialize_state = StepStatus::InProgress,
        }
    }

    /// Marks a step as completed
    pub fn complete_step(&mut self, step: WizardStep) {
        match step {
            WizardStep::FetchDetails => self.fetch_details = StepStatus::Completed,
            WizardStep::CheckPrerequisites => self.check_prerequisites = StepStatus::Completed,
            WizardStep::FetchTargetBranch => self.fetch_target_branch = StepStatus::Completed,
            WizardStep::CloneOrWorktree => self.clone_or_worktree = StepStatus::Completed,
            WizardStep::ConfigureRepository => self.configure_repository = StepStatus::Completed,
            WizardStep::CreateBranch => self.create_branch = StepStatus::Completed,
            WizardStep::PrepareCherryPicks => self.prepare_cherry_picks = StepStatus::Completed,
            WizardStep::InitializeState => self.initialize_state = StepStatus::Completed,
        }
        if self.current_step == Some(step) {
            self.current_step = None;
        }
    }

    /// Returns the current step's progress message
    pub fn current_message(&self) -> String {
        match self.current_step {
            Some(step) => step.progress_message(self.is_clone_mode).to_string(),
            None => "Initializing...".to_string(),
        }
    }

    /// Returns whether in clone mode
    pub fn is_clone_mode(&self) -> bool {
        self.is_clone_mode
    }
}

/// Intermediate data stored between wizard steps (accumulated from StepResults)
#[derive(Debug, Clone, Default)]
pub struct StepData {
    /// SSH URL fetched from Azure DevOps (clone mode)
    pub ssh_url: Option<String>,
    /// Repository path after clone/worktree creation
    pub repo_path: Option<PathBuf>,
    /// Whether this is a worktree (vs clone)
    pub is_worktree: bool,
    /// Base repo path for worktree cleanup
    pub base_repo_path: Option<PathBuf>,
    /// Branch name to create
    pub branch_name: Option<String>,
    /// Cherry-pick items prepared during setup
    pub cherry_pick_items: Option<Vec<CherryPickItem>>,
}

impl StepData {
    /// Merge a StepResult into this StepData, updating any fields that are set.
    pub fn merge_result(&mut self, result: &StepResult) {
        if result.ssh_url.is_some() {
            self.ssh_url = result.ssh_url.clone();
        }
        if result.repo_path.is_some() {
            self.repo_path = result.repo_path.clone();
        }
        if result.is_worktree {
            self.is_worktree = true;
        }
        if result.base_repo_path.is_some() {
            self.base_repo_path = result.base_repo_path.clone();
        }
        if result.branch_name.is_some() {
            self.branch_name = result.branch_name.clone();
        }
        if result.cherry_pick_items.is_some() {
            self.cherry_pick_items = result.cherry_pick_items.clone();
        }
    }
}

/// Internal state of the setup wizard
#[derive(Debug)]
pub enum SetupState {
    /// Initial state before starting
    Initializing,
    /// Background task is running, receiving progress updates via channel
    Running {
        progress: WizardProgress,
        /// Accumulated step data from completed steps
        step_data: StepData,
    },
    /// All steps completed successfully
    Complete {
        /// Final step data with all accumulated results
        step_data: StepData,
        /// Cherry-pick items prepared during setup
        cherry_pick_items: Vec<CherryPickItem>,
    },
    /// An error occurred during setup
    Error {
        error: git::RepositorySetupError,
        message: String,
        /// Progress at the time of error (to show which step failed)
        progress: Option<WizardProgress>,
    },
}

/// Channel receiver wrapper that allows Debug implementation
struct ProgressReceiver(mpsc::Receiver<ProgressMessage>);

impl std::fmt::Debug for ProgressReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressReceiver").finish_non_exhaustive()
    }
}

/// The setup repository state machine
pub struct SetupRepoState {
    state: SetupState,
    /// Channel receiver for progress messages from background task
    receiver: Option<ProgressReceiver>,
    /// Cached mode detection (None until first run)
    is_clone_mode: Option<bool>,
}

impl std::fmt::Debug for SetupRepoState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupRepoState")
            .field("state", &self.state)
            .field("is_clone_mode", &self.is_clone_mode)
            .finish_non_exhaustive()
    }
}

impl Default for SetupRepoState {
    fn default() -> Self {
        Self::new()
    }
}

impl SetupRepoState {
    pub fn new() -> Self {
        Self {
            state: SetupState::Initializing,
            receiver: None,
            is_clone_mode: None,
        }
    }

    /// Get mutable reference to progress if in running state
    fn progress_mut(&mut self) -> Option<&mut WizardProgress> {
        match &mut self.state {
            SetupState::Running { progress, .. } => Some(progress),
            _ => None,
        }
    }

    /// Start a wizard step
    fn start_step(&mut self, step: WizardStep) {
        if let Some(progress) = self.progress_mut() {
            progress.start_step(step);
        }
    }

    /// Complete a wizard step
    fn complete_step(&mut self, step: WizardStep) {
        if let Some(progress) = self.progress_mut() {
            progress.complete_step(step);
        }
    }

    /// Merge a step result into the accumulated step data
    fn merge_step_result(&mut self, result: &StepResult) {
        if let SetupState::Running { step_data, .. } = &mut self.state {
            step_data.merge_result(result);
        }
    }

    fn set_error(&mut self, error: SetupError) {
        // Preserve the current progress to show which step failed
        let current_progress = match &self.state {
            SetupState::Running { progress, .. } => Some(progress.clone()),
            _ => None,
        };

        let message = match &error {
            SetupError::BranchExists(branch) => {
                format!(
                    "Branch '{}' already exists.\n\nThis can happen if you've run this tool before or if the branch was created elsewhere.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force delete the branch and continue\n  • Press 'Esc' to go back",
                    branch
                )
            }
            SetupError::WorktreeExists(path) => {
                format!(
                    "Worktree already exists at:\n{}\n\nThis can happen if you've run this tool before or if the worktree was created elsewhere.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force remove the worktree and continue\n  • Press 'Esc' to go back",
                    path
                )
            }
            SetupError::Other(msg) => {
                format!(
                    "Setup failed: {}\n\nOptions:\n  • Press 'r' to retry\n  • Press 'Esc' to go back",
                    msg
                )
            }
        };
        self.state = SetupState::Error {
            error: error.into(),
            message,
            progress: current_progress,
        };
    }

    /// Start the background setup task and return the progress receiver
    fn start_background_task(&mut self, ctx: SetupContext) {
        let (tx, rx) = mpsc::channel::<ProgressMessage>(32);
        self.receiver = Some(ProgressReceiver(rx));
        self.is_clone_mode = Some(ctx.is_clone_mode);

        // Initialize the Running state
        self.state = SetupState::Running {
            progress: WizardProgress::new(ctx.is_clone_mode),
            step_data: StepData::default(),
        };

        // Spawn the background task
        tokio::spawn(run_setup_task(ctx, tx));
    }

    /// Process a message received from the background task
    fn handle_progress_message(&mut self, msg: ProgressMessage) {
        match msg {
            ProgressMessage::StepStarted(step) => {
                self.start_step(step);
            }
            ProgressMessage::StepCompleted(step, result) => {
                self.complete_step(step);
                self.merge_step_result(&result);
            }
            ProgressMessage::AllComplete => {
                // Extract the accumulated data and transition to Complete state
                if let SetupState::Running { step_data, .. } = &self.state {
                    // Get cherry-pick items from accumulated step_data
                    let cherry_pick_items = step_data.cherry_pick_items.clone().unwrap_or_default();
                    self.state = SetupState::Complete {
                        step_data: step_data.clone(),
                        cherry_pick_items,
                    };
                }
            }
            ProgressMessage::Error(err) => {
                self.set_error(err);
            }
        }
    }

    /// Apply the completed setup results to the MergeApp
    fn apply_results_to_app(&self, app: &mut MergeApp, cherry_pick_items: Vec<CherryPickItem>) {
        if let SetupState::Complete { step_data, .. } = &self.state {
            // Set repo path
            if let Some(repo_path) = &step_data.repo_path {
                app.set_repo_path(Some(repo_path.clone()));
            }

            // Set base repo path for worktree mode
            if let Some(base_path) = &step_data.base_repo_path {
                app.worktree.base_repo_path = Some(base_path.clone());
            }

            // Set cherry-pick items
            *app.cherry_pick_items_mut() = cherry_pick_items;

            // State file is created during InitializeState step in background task
            // via the shared StateManager, so no need to create it here
        }
    }

    async fn force_resolve_error(
        &mut self,
        app: &mut MergeApp,
        error: git::RepositorySetupError,
    ) -> StateChange<MergeState> {
        let version = app.version().unwrap();

        match error {
            git::RepositorySetupError::BranchExists(branch_name) => {
                // Force delete the branch before retrying
                // Try repo_path first (for worktree case after creation), then local_repo
                let delete_path = app
                    .repo_path()
                    .or_else(|| app.local_repo().map(std::path::Path::new));

                if let Some(repo_path) = delete_path
                    && let Err(e) = git::force_delete_branch(repo_path, &branch_name)
                {
                    app.set_error_message(Some(format!("Failed to force delete branch: {}", e)));
                    return StateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
            git::RepositorySetupError::WorktreeExists(_) => {
                // Force remove the worktree before retrying
                if let Some(repo_path) = app.local_repo()
                    && let Err(e) =
                        git::force_remove_worktree(std::path::Path::new(repo_path), version)
                {
                    app.set_error_message(Some(format!("Failed to force remove worktree: {}", e)));
                    return StateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
            git::RepositorySetupError::Other(_) => {
                // For other errors, just retry
            }
        }

        // After force operation, reset and restart the setup
        self.state = SetupState::Initializing;
        self.receiver = None;
        StateChange::Keep
    }
}

// ============================================================================
// Background Task Implementation
// ============================================================================

/// Runs the setup steps in a background task, sending progress updates via channel.
///
/// This function executes all wizard steps sequentially, sending progress messages
/// to the UI through the provided channel. The UI can then update the display
/// as each step starts and completes.
async fn run_setup_task(ctx: SetupContext, tx: mpsc::Sender<ProgressMessage>) {
    // Accumulated data passed between steps
    let mut ssh_url: Option<String> = None;
    let mut repo_path: Option<PathBuf> = None;
    let mut base_repo_path: Option<PathBuf> = None;
    let mut is_worktree = false;
    let mut branch_name: Option<String> = None;

    // Determine the sequence of steps based on mode
    let steps: Vec<WizardStep> = if ctx.is_clone_mode {
        vec![
            WizardStep::FetchDetails,
            WizardStep::CheckPrerequisites,
            WizardStep::CloneOrWorktree,
            WizardStep::ConfigureRepository,
            WizardStep::CreateBranch,
            WizardStep::PrepareCherryPicks,
            WizardStep::InitializeState,
        ]
    } else {
        vec![
            WizardStep::CheckPrerequisites,
            WizardStep::FetchTargetBranch,
            WizardStep::CloneOrWorktree,
            WizardStep::ConfigureRepository,
            WizardStep::CreateBranch,
            WizardStep::PrepareCherryPicks,
            WizardStep::InitializeState,
        ]
    };

    for step in steps {
        // Send step started message
        if tx.send(ProgressMessage::StepStarted(step)).await.is_err() {
            return; // Channel closed, UI no longer listening
        }

        // Execute the step
        let result = execute_step_impl(
            step,
            &ctx,
            &mut ssh_url,
            &mut repo_path,
            &mut base_repo_path,
            &mut is_worktree,
            &mut branch_name,
        )
        .await;

        match result {
            Ok(step_result) => {
                // Send step completed message
                if tx
                    .send(ProgressMessage::StepCompleted(step, step_result))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(err) => {
                // Send error message and stop
                let _ = tx.send(ProgressMessage::Error(err)).await;
                return;
            }
        }
    }

    // All steps completed successfully
    let _ = tx.send(ProgressMessage::AllComplete).await;
}

/// Execute a single setup step and return the result.
async fn execute_step_impl(
    step: WizardStep,
    ctx: &SetupContext,
    ssh_url: &mut Option<String>,
    repo_path: &mut Option<PathBuf>,
    base_repo_path: &mut Option<PathBuf>,
    is_worktree: &mut bool,
    branch_name: &mut Option<String>,
) -> Result<StepResult, SetupError> {
    match step {
        WizardStep::FetchDetails => {
            // Clone mode: fetch SSH URL from Azure DevOps
            match ctx.client.fetch_repo_details().await {
                Ok(details) => {
                    *ssh_url = Some(details.ssh_url.clone());
                    Ok(StepResult {
                        ssh_url: Some(details.ssh_url),
                        ..Default::default()
                    })
                }
                Err(e) => Err(SetupError::Other(format!(
                    "Failed to fetch repository details: {}",
                    e
                ))),
            }
        }

        WizardStep::CheckPrerequisites => {
            if ctx.is_clone_mode {
                // For clone mode, verify we have the SSH URL
                if ssh_url.is_none() {
                    return Err(SetupError::Other("SSH URL not available".to_string()));
                }
                // Note: Cannot check branch existence in clone mode - no local repo yet
            } else {
                // For worktree mode
                if let Some(local_repo) = &ctx.local_repo {
                    let base_path = std::path::Path::new(local_repo);

                    // Check 1: Verify local repo path exists (unrecoverable)
                    if !base_path.exists() {
                        return Err(SetupError::Other(format!(
                            "Local repository path does not exist: {:?}",
                            base_path
                        )));
                    }

                    // Check 2: Verify worktree doesn't already exist (recoverable via 'f')
                    match git::worktree_exists(base_path, &ctx.version) {
                        Ok(true) => {
                            let worktree_path = base_path.join(format!("next-{}", ctx.version));
                            return Err(SetupError::WorktreeExists(
                                worktree_path.display().to_string(),
                            ));
                        }
                        Ok(false) => {}
                        Err(e) => {
                            return Err(SetupError::Other(format!(
                                "Failed to check worktree existence: {}",
                                e
                            )));
                        }
                    }

                    // Check 3: Verify patch branch doesn't already exist (recoverable via 'f')
                    let branch_name = format!("patch/{}-{}", ctx.target_branch, ctx.version);
                    match git::branch_exists(base_path, &branch_name) {
                        Ok(true) => {
                            return Err(SetupError::BranchExists(branch_name));
                        }
                        Ok(false) => {}
                        Err(e) => {
                            return Err(SetupError::Other(format!(
                                "Failed to check branch existence: {}",
                                e
                            )));
                        }
                    }
                } else {
                    return Err(SetupError::Other(
                        "Local repository path not set for worktree mode".to_string(),
                    ));
                }
            }
            Ok(StepResult::default())
        }

        WizardStep::FetchTargetBranch => {
            // Worktree mode: fetch target branch from remote
            if let Some(local_repo) = &ctx.local_repo {
                let output = std::process::Command::new("git")
                    .current_dir(local_repo)
                    .args(["fetch", "origin", &ctx.target_branch])
                    .output();

                match output {
                    Ok(result) if result.status.success() => Ok(StepResult::default()),
                    Ok(result) => Err(SetupError::Other(format!(
                        "Failed to fetch target branch: {}",
                        String::from_utf8_lossy(&result.stderr)
                    ))),
                    Err(e) => Err(SetupError::Other(format!(
                        "Failed to fetch target branch: {}",
                        e
                    ))),
                }
            } else {
                Err(SetupError::Other(
                    "Local repository path not set".to_string(),
                ))
            }
        }

        WizardStep::CloneOrWorktree => {
            if ctx.is_clone_mode {
                // Clone mode
                let url = ssh_url.clone().unwrap_or_default();
                match git::shallow_clone_repo(&url, &ctx.target_branch, ctx.run_hooks) {
                    Ok((path, _temp_dir)) => {
                        // Note: temp_dir ownership is tricky across threads
                        // For now, we leak it (it will be cleaned up on process exit)
                        // A better solution would be to pass it back through the channel
                        *repo_path = Some(path.clone());
                        *is_worktree = false;
                        Ok(StepResult {
                            repo_path: Some(path),
                            is_worktree: false,
                            ..Default::default()
                        })
                    }
                    Err(e) => Err(SetupError::Other(e.to_string())),
                }
            } else {
                // Worktree mode
                if let Some(local_repo) = &ctx.local_repo {
                    let base_path = PathBuf::from(local_repo);
                    match git::create_worktree(
                        &base_path,
                        &ctx.target_branch,
                        &ctx.version,
                        ctx.run_hooks,
                    ) {
                        Ok(worktree_path) => {
                            *repo_path = Some(worktree_path.clone());
                            *base_repo_path = Some(base_path.clone());
                            *is_worktree = true;
                            Ok(StepResult {
                                repo_path: Some(worktree_path),
                                is_worktree: true,
                                base_repo_path: Some(base_path),
                                ..Default::default()
                            })
                        }
                        Err(e) => Err(e.into()),
                    }
                } else {
                    Err(SetupError::Other(
                        "Local repository path not set".to_string(),
                    ))
                }
            }
        }

        WizardStep::ConfigureRepository => {
            // Configure the repository (disable hooks unless --run-hooks is specified)
            if let Some(path) = repo_path {
                if !ctx.run_hooks {
                    let output = std::process::Command::new("git")
                        .current_dir(path)
                        .args(["config", "core.hooksPath", "/dev/null"])
                        .output();

                    match output {
                        Ok(result) if result.status.success() => Ok(StepResult::default()),
                        Ok(result) => Err(SetupError::Other(format!(
                            "Failed to configure hooks path: {}",
                            String::from_utf8_lossy(&result.stderr)
                        ))),
                        Err(e) => Err(SetupError::Other(format!(
                            "Failed to configure hooks path: {}",
                            e
                        ))),
                    }
                } else {
                    // Hooks enabled, nothing to configure
                    Ok(StepResult::default())
                }
            } else {
                Err(SetupError::Other("Repository path not set".to_string()))
            }
        }

        WizardStep::CreateBranch => {
            let name = format!("patch/{}-{}", ctx.target_branch, ctx.version);
            if let Some(path) = repo_path {
                match git::create_branch(path, &name) {
                    Ok(()) => {
                        *branch_name = Some(name.clone());
                        Ok(StepResult {
                            branch_name: Some(name),
                            ..Default::default()
                        })
                    }
                    // Note: Branch existence is checked earlier in CheckPrerequisites step
                    Err(e) => Err(SetupError::Other(format!("Failed to create branch: {}", e))),
                }
            } else {
                Err(SetupError::Other("Repository path not set".to_string()))
            }
        }

        WizardStep::PrepareCherryPicks => {
            // Prepare cherry-pick items from selected PRs
            let cherry_pick_items: Vec<CherryPickItem> = ctx
                .selected_prs
                .iter()
                .filter_map(|pr| {
                    pr.merge_commit_id.as_ref().map(|commit_id| CherryPickItem {
                        commit_id: commit_id.clone(),
                        pr_id: pr.pr_id,
                        pr_title: pr.pr_title.clone(),
                        status: crate::models::CherryPickStatus::Pending,
                    })
                })
                .collect();

            if cherry_pick_items.is_empty() {
                return Err(SetupError::Other(
                    "No commits found to cherry-pick".to_string(),
                ));
            }

            Ok(StepResult {
                cherry_pick_items: Some(cherry_pick_items),
                ..Default::default()
            })
        }

        WizardStep::InitializeState => {
            // Create state file via StateManager (shared via Arc<Mutex<>>)
            if let Some(repo_path_ref) = repo_path {
                let mut manager = ctx.state_manager.lock().unwrap();
                manager
                    .create_state_file(
                        repo_path_ref.clone(),
                        base_repo_path.clone(),
                        *is_worktree,
                        &ctx.version,
                        &ctx.state_config,
                    )
                    .map_err(|e| {
                        SetupError::Other(format!("Failed to create state file: {}", e))
                    })?;

                // Set initial phase to CherryPicking
                manager
                    .update_phase(MergePhase::CherryPicking)
                    .map_err(|e| SetupError::Other(format!("Failed to update phase: {}", e)))?;
            }
            Ok(StepResult::default())
        }
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

/// Renders the wizard step indicator showing all steps with their status
fn render_step_indicator(f: &mut Frame, area: Rect, progress: &WizardProgress) {
    let steps = progress.steps();
    let total_steps = steps.len();

    // Build the step indicator spans
    let mut spans: Vec<Span> = Vec::new();

    for (i, (step, status)) in steps.iter().enumerate() {
        let step_num = i + 1;
        let step_name = step.display_name(progress.is_clone_mode());

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

        // Add step: "1 ✓ Fetch Details"
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
fn render_current_step_progress(f: &mut Frame, area: Rect, progress: &WizardProgress) {
    let message = progress.current_message();

    // Build content with current step message
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            message,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    // Add a visual spinner/progress indicator
    lines.push(Line::from(Span::styled(
        "Please wait...",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Current Step")
            .title_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(paragraph, area);
}

#[async_trait]
impl ModeState for SetupRepoState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Length(3), // Step indicator
                Constraint::Min(5),    // Current step progress
            ])
            .split(f.area());

        // Title - color changes based on state
        let (title_text, title_color) = match &self.state {
            SetupState::Error { .. } => ("Repository Setup - Error", Color::Red),
            _ => ("Repository Setup", Color::Green),
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
            SetupState::Initializing => {
                // Show default step indicator for initial state
                // Determine mode from app context
                let is_clone_mode = app.local_repo().is_none();
                let progress = WizardProgress::new(is_clone_mode);

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
            SetupState::Running { progress, .. } => {
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
            SetupState::Complete { .. } => {
                // This state is transient - we transition to CherryPick immediately
                // But render a completion message just in case
                let is_clone_mode = self.is_clone_mode.unwrap_or(app.local_repo().is_none());
                let progress = WizardProgress::new(is_clone_mode);

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
                        "Setup complete!",
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
            SetupState::Error {
                message, progress, ..
            } => {
                // Show step indicator with error state - use preserved progress if available
                let display_progress = match progress {
                    Some(p) => p.clone(),
                    None => {
                        let is_clone_mode =
                            self.is_clone_mode.unwrap_or(app.local_repo().is_none());
                        WizardProgress::new(is_clone_mode)
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

                // Error message
                let key_style = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);

                // Helper to style hotkeys in a line like "  • Press 'r' to retry"
                fn style_hotkey_line<'a>(line: &'a str, key_style: Style) -> Line<'a> {
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

                let message_lines: Vec<Line> = message
                    .lines()
                    .map(|line| {
                        if line.starts_with("Options:") {
                            Line::from(vec![Span::styled(line, Style::default().fg(Color::Cyan))])
                        } else if line.starts_with("  •") {
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
        // We collect them first to avoid borrowing issues with self
        let messages: Vec<ProgressMessage> =
            if let Some(ProgressReceiver(ref mut rx)) = self.receiver {
                let mut msgs = Vec::new();
                while let Ok(msg) = rx.try_recv() {
                    msgs.push(msg);
                }
                msgs
            } else {
                Vec::new()
            };

        // Process collected messages
        for msg in messages {
            self.handle_progress_message(msg);
        }

        match &self.state {
            SetupState::Error { error, .. } => {
                match code {
                    KeyCode::Char('r' | 'R') => {
                        // Retry - reset state and try again
                        self.state = SetupState::Initializing;
                        self.receiver = None;
                        StateChange::Keep
                    }
                    KeyCode::Char('f' | 'F') => {
                        // Force - try to resolve the specific error and retry
                        let error_clone = error.clone();
                        self.force_resolve_error(app, error_clone).await
                    }
                    KeyCode::Esc => {
                        // Go back to previous state or exit
                        StateChange::Change(MergeState::Error(ErrorState::new()))
                    }
                    _ => StateChange::Keep,
                }
            }
            SetupState::Initializing => {
                // Extract context and start background task
                if let Some(ctx) = SetupContext::from_app(app) {
                    self.start_background_task(ctx);
                } else {
                    app.set_error_message(Some(
                        "Failed to extract setup context (missing version?)".to_string(),
                    ));
                    return StateChange::Change(MergeState::Error(ErrorState::new()));
                }
                StateChange::Keep
            }
            SetupState::Running { .. } => {
                // Background task is running, just keep state for re-render
                // Messages are processed at the start of process_key
                StateChange::Keep
            }
            SetupState::Complete {
                cherry_pick_items, ..
            } => {
                // Apply results to app and transition to CherryPick state
                let items = cherry_pick_items.clone();
                self.apply_results_to_app(app, items);
                StateChange::Change(MergeState::CherryPick(CherryPickState::new()))
            }
        }
    }

    fn name(&self) -> &'static str {
        "SetupRepo"
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

    /// Creates a test SetupContext with the given run_hooks setting.
    fn create_test_setup_context(run_hooks: bool) -> SetupContext {
        SetupContext {
            client: crate::api::AzureDevOpsClient::new(
                "https://dev.azure.com/org".to_string(),
                "project".to_string(),
                "repo".to_string(),
                "pat".to_string(),
            )
            .unwrap(),
            is_clone_mode: true,
            local_repo: None,
            target_branch: "main".to_string(),
            version: "1.0.0".to_string(),
            run_hooks,
            selected_prs: vec![],
            state_manager: Arc::new(Mutex::new(StateManager::new())),
            state_config: StateCreateConfig {
                organization: "org".to_string(),
                project: "project".to_string(),
                repository: "repo".to_string(),
                dev_branch: "dev".to_string(),
                target_branch: "main".to_string(),
                tag_prefix: "merged/".to_string(),
                work_item_state: "Done".to_string(),
                run_hooks,
            },
        }
    }

    /// # Setup Repo State - Initializing
    ///
    /// Tests the repository setup screen in initial state.
    ///
    /// ## Test Scenario
    /// - Creates a new setup repo state
    /// - Renders the state in initializing stage
    ///
    /// ## Expected Outcome
    /// - Should display "Initializing repository..." message
    /// - Should show "Repository Setup" title
    /// - Should use yellow styling
    #[test]
    fn test_setup_repo_initializing() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = MergeState::SetupRepo(SetupRepoState::new());
            harness.render_merge_state(&mut state);

            assert_snapshot!("initializing", harness.backend());
        });
    }

    /// Helper to create a Running state with given progress
    fn make_running(progress: WizardProgress) -> SetupState {
        SetupState::Running {
            progress,
            step_data: StepData::default(),
        }
    }

    /// # Setup Repo State - Cloning (Fetch Details Step)
    ///
    /// Tests the repository setup screen during fetching details (clone mode).
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with clone mode progress
    /// - Sets current step to FetchDetails
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps at top with FetchDetails highlighted
    /// - Should show "Fetching repository details..." message
    #[test]
    fn test_setup_repo_fetch_details() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(true); // clone mode
            progress.start_step(WizardStep::FetchDetails);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(true);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("fetch_details", harness.backend());
        });
    }

    /// # Setup Repo State - Cloning (Check Prerequisites Step)
    ///
    /// Tests the repository setup screen during check prerequisites (clone mode).
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with clone mode progress
    /// - Sets current step to CheckPrerequisites after completing FetchDetails
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps with FetchDetails completed and CheckPrerequisites active
    /// - Should show "Checking prerequisites..." message
    #[test]
    fn test_setup_repo_check_prerequisites() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(true); // clone mode
            progress.complete_step(WizardStep::FetchDetails);
            progress.start_step(WizardStep::CheckPrerequisites);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(true);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("check_prerequisites", harness.backend());
        });
    }

    /// # Setup Repo State - Cloning Repository
    ///
    /// Tests the repository setup screen during cloning.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with clone mode progress
    /// - Sets current step to CloneOrWorktree after completing previous steps
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps with previous steps completed and CloneOrWorktree active
    /// - Should show "Cloning repository..." message
    #[test]
    fn test_setup_repo_cloning() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(true); // clone mode
            progress.complete_step(WizardStep::FetchDetails);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.start_step(WizardStep::CloneOrWorktree);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(true);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("cloning", harness.backend());
        });
    }

    /// # Setup Repo State - Fetching Target Branch (Worktree Mode)
    ///
    /// Tests the repository setup screen during target branch fetching.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with worktree mode progress
    /// - Sets current step to FetchTargetBranch after completing CheckPrerequisites
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps with CheckPrerequisites completed and FetchTargetBranch active
    /// - Should show "Fetching target branch..." message
    #[test]
    fn test_setup_repo_fetch_target_branch() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(false); // worktree mode
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.start_step(WizardStep::FetchTargetBranch);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(false);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("fetch_target_branch", harness.backend());
        });
    }

    /// # Setup Repo State - Creating Worktree
    ///
    /// Tests the repository setup screen during worktree creation.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with worktree mode progress
    /// - Sets current step to CloneOrWorktree (worktree mode)
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps with CloneOrWorktree active (no FetchDetails step)
    /// - Should show "Creating worktree..." message
    #[test]
    fn test_setup_repo_creating_worktree() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(false); // worktree mode
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.start_step(WizardStep::CloneOrWorktree);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(false);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("creating_worktree", harness.backend());
        });
    }

    /// # Setup Repo State - Creating Branch
    ///
    /// Tests the repository setup screen during branch creation.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with worktree mode progress
    /// - Sets current step to CreateBranch after completing previous steps
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps with previous steps completed and CreateBranch active
    /// - Should show "Creating patch branch..." message
    #[test]
    fn test_setup_repo_creating_branch() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(false); // worktree mode
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.complete_step(WizardStep::ConfigureRepository);
            progress.start_step(WizardStep::CreateBranch);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(false);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("creating_branch", harness.backend());
        });
    }

    /// # Setup Repo State - Preparing Cherry-Picks
    ///
    /// Tests the repository setup screen during cherry-pick preparation.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with worktree mode progress
    /// - Sets current step to PrepareCherryPicks after completing previous steps
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps with previous steps completed and PrepareCherryPicks active
    /// - Should show "Preparing cherry-pick items..." message
    #[test]
    fn test_setup_repo_preparing_cherry_picks() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(false); // worktree mode
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.complete_step(WizardStep::ConfigureRepository);
            progress.complete_step(WizardStep::CreateBranch);
            progress.start_step(WizardStep::PrepareCherryPicks);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(false);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("preparing_cherry_picks", harness.backend());
        });
    }

    /// # Setup Repo State - Initializing State File
    ///
    /// Tests the repository setup screen during state file initialization.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with worktree mode progress
    /// - Sets current step to InitializeState after completing previous steps
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display wizard steps with previous steps completed and InitializeState active
    /// - Should show "Initializing state file..." message
    #[test]
    fn test_setup_repo_initializing_state() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(false); // worktree mode
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.complete_step(WizardStep::ConfigureRepository);
            progress.complete_step(WizardStep::CreateBranch);
            progress.complete_step(WizardStep::PrepareCherryPicks);
            progress.start_step(WizardStep::InitializeState);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(false);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("initializing_state", harness.backend());
        });
    }

    /// # Setup Repo State - Clone Mode All Steps Complete
    ///
    /// Tests the repository setup screen with all steps completed in clone mode.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with clone mode progress
    /// - Completes all steps in clone mode
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display all steps with checkmarks
    /// - All steps should be green/completed
    #[test]
    fn test_setup_repo_clone_mode_all_complete() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            let mut progress = WizardProgress::new(true); // clone mode
            progress.complete_step(WizardStep::FetchDetails);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.complete_step(WizardStep::ConfigureRepository);
            progress.complete_step(WizardStep::CreateBranch);
            progress.complete_step(WizardStep::PrepareCherryPicks);
            progress.complete_step(WizardStep::InitializeState);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(true);
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("clone_mode_all_complete", harness.backend());
        });
    }

    /// # Setup Repo State - Error With Progress Preserved
    ///
    /// Tests that error state preserves and displays the progress at time of failure.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with progress (some steps completed)
    /// - Triggers an error state
    /// - Renders the error display
    ///
    /// ## Expected Outcome
    /// - Should show completed steps with checkmarks
    /// - Should show the failing step (last in-progress) highlighted
    /// - Should display error message
    #[test]
    fn test_setup_repo_error_with_progress() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            // Simulate progress: fetch and check completed, cloning in progress when error occurred
            let mut progress = WizardProgress::new(true); // clone mode
            progress.complete_step(WizardStep::FetchDetails);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.start_step(WizardStep::CloneOrWorktree);
            inner_state.state = make_running(progress);
            inner_state.is_clone_mode = Some(true);

            // Now trigger an error (this preserves the progress)
            inner_state.set_error(SetupError::from(git::RepositorySetupError::WorktreeExists(
                "/path/to/repo/.worktrees/v1.0.0".to_string(),
            )));

            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("error_with_progress", harness.backend());
        });
    }

    /// # Setup Repo State - Branch Exists Error
    ///
    /// Tests the error display when a branch already exists.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets an error for existing branch
    /// - Renders the error display
    ///
    /// ## Expected Outcome
    /// - Should display error message with branch name
    /// - Should show options (retry, force, go back)
    /// - Should use red styling for title
    /// - Should have different colors for different text sections
    #[test]
    fn test_setup_repo_branch_exists_error() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            inner_state.set_error(SetupError::from(git::RepositorySetupError::BranchExists(
                "patch/main-v1.0.0".to_string(),
            )));
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("branch_exists_error", harness.backend());
        });
    }

    /// # Setup Repo State - Worktree Exists Error
    ///
    /// Tests the error display when a worktree already exists.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets an error for existing worktree
    /// - Renders the error display
    ///
    /// ## Expected Outcome
    /// - Should display error message with worktree path
    /// - Should show options (retry, force, go back)
    #[test]
    fn test_setup_repo_worktree_exists_error() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            inner_state.set_error(SetupError::from(git::RepositorySetupError::WorktreeExists(
                "/path/to/repo/.worktrees/v1.0.0".to_string(),
            )));
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("worktree_exists_error", harness.backend());
        });
    }

    /// # Setup Repo State - Other Error
    ///
    /// Tests the error display for generic errors.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets a generic error
    /// - Renders the error display
    ///
    /// ## Expected Outcome
    /// - Should display error message
    /// - Should show retry and go back options
    #[test]
    fn test_setup_repo_other_error() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner_state = SetupRepoState::new();
            inner_state.set_error(SetupError::from(git::RepositorySetupError::Other(
                "Failed to fetch repository details from Azure DevOps".to_string(),
            )));
            let mut state = MergeState::SetupRepo(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("other_error", harness.backend());
        });
    }

    /// # SetupRepoState Default Implementation
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates SetupRepoState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should initialize with Initializing state and receiver=None
    #[test]
    fn test_setup_repo_default() {
        let state = SetupRepoState::default();
        assert!(state.receiver.is_none());
        assert!(matches!(state.state, SetupState::Initializing));
    }

    /// # Setup Repo State - Escape Key in Error State
    ///
    /// Tests Escape key handling in error state.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with error
    /// - Processes Escape key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change (to ErrorState)
    #[tokio::test]
    async fn test_setup_repo_escape_in_error() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = SetupRepoState::new();
        state.set_error(SetupError::Other("Test error".to_string()));

        let result =
            ModeState::process_key(&mut state, KeyCode::Esc, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # Setup Repo State - Other Keys in Error State
    ///
    /// Tests that unrecognized keys are ignored in error state.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state with error
    /// - Processes various unrecognized keys
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_setup_repo_other_keys_in_error() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = SetupRepoState::new();
        state.set_error(SetupError::Other("Test error".to_string()));

        for key in [KeyCode::Up, KeyCode::Down, KeyCode::Char('x')] {
            let result = ModeState::process_key(&mut state, key, harness.merge_app_mut()).await;
            assert!(matches!(result, StateChange::Keep));
        }
    }

    /// # Setup Repo State - Key in Running State
    ///
    /// Tests key handling when setup is running (background task active).
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state in Running state
    /// - Processes a key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep (background task running)
    #[tokio::test]
    async fn test_setup_repo_key_when_running() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = SetupRepoState::new();
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            step_data: StepData::default(),
        };

        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    // =========================================================================
    // Unit Tests for Message Types
    // =========================================================================

    /// # ProgressMessage - StepStarted
    ///
    /// Tests that StepStarted messages can be created and matched.
    #[test]
    fn test_progress_message_step_started() {
        let msg = ProgressMessage::StepStarted(WizardStep::FetchDetails);
        assert!(matches!(
            msg,
            ProgressMessage::StepStarted(WizardStep::FetchDetails)
        ));
    }

    /// # ProgressMessage - StepCompleted
    ///
    /// Tests that StepCompleted messages carry step result data.
    #[test]
    fn test_progress_message_step_completed() {
        let result = StepResult {
            ssh_url: Some("git@example.com:repo.git".to_string()),
            ..Default::default()
        };
        let msg = ProgressMessage::StepCompleted(WizardStep::FetchDetails, result);
        if let ProgressMessage::StepCompleted(step, res) = msg {
            assert_eq!(step, WizardStep::FetchDetails);
            assert_eq!(res.ssh_url, Some("git@example.com:repo.git".to_string()));
        } else {
            panic!("Expected StepCompleted");
        }
    }

    /// # ProgressMessage - AllComplete
    ///
    /// Tests the AllComplete message variant.
    #[test]
    fn test_progress_message_all_complete() {
        let msg = ProgressMessage::AllComplete;
        assert!(matches!(msg, ProgressMessage::AllComplete));
    }

    /// # ProgressMessage - Error
    ///
    /// Tests error messages with different error types.
    #[test]
    fn test_progress_message_error() {
        let msg = ProgressMessage::Error(SetupError::BranchExists("main".to_string()));
        if let ProgressMessage::Error(SetupError::BranchExists(branch)) = msg {
            assert_eq!(branch, "main");
        } else {
            panic!("Expected Error with BranchExists");
        }
    }

    /// # StepResult - Default
    ///
    /// Tests that StepResult::default() creates empty result.
    #[test]
    fn test_step_result_default() {
        let result = StepResult::default();
        assert!(result.ssh_url.is_none());
        assert!(result.repo_path.is_none());
        assert!(!result.is_worktree);
        assert!(result.base_repo_path.is_none());
        assert!(result.branch_name.is_none());
        assert!(result.cherry_pick_items.is_none());
    }

    /// # StepData - Merge Result
    ///
    /// Tests that StepData correctly merges StepResult fields.
    #[test]
    fn test_step_data_merge_result() {
        let mut data = StepData::default();

        // First merge: set ssh_url
        let result1 = StepResult {
            ssh_url: Some("git@example.com:repo.git".to_string()),
            ..Default::default()
        };
        data.merge_result(&result1);
        assert_eq!(data.ssh_url, Some("git@example.com:repo.git".to_string()));

        // Second merge: set repo_path, preserving ssh_url
        let result2 = StepResult {
            repo_path: Some(PathBuf::from("/tmp/repo")),
            is_worktree: true,
            ..Default::default()
        };
        data.merge_result(&result2);
        assert_eq!(data.ssh_url, Some("git@example.com:repo.git".to_string()));
        assert_eq!(data.repo_path, Some(PathBuf::from("/tmp/repo")));
        assert!(data.is_worktree);
    }

    /// # SetupError - From RepositorySetupError
    ///
    /// Tests conversion from git::RepositorySetupError to SetupError.
    #[test]
    fn test_setup_error_from_repository_setup_error() {
        let branch_err = git::RepositorySetupError::BranchExists("main".to_string());
        let setup_err: SetupError = branch_err.into();
        assert!(matches!(setup_err, SetupError::BranchExists(ref b) if b == "main"));

        let worktree_err = git::RepositorySetupError::WorktreeExists("/path".to_string());
        let setup_err: SetupError = worktree_err.into();
        assert!(matches!(setup_err, SetupError::WorktreeExists(ref p) if p == "/path"));

        let other_err = git::RepositorySetupError::Other("error".to_string());
        let setup_err: SetupError = other_err.into();
        assert!(matches!(setup_err, SetupError::Other(ref m) if m == "error"));
    }

    /// # SetupError - Into RepositorySetupError
    ///
    /// Tests conversion from SetupError back to git::RepositorySetupError.
    #[test]
    fn test_setup_error_into_repository_setup_error() {
        let setup_err = SetupError::BranchExists("main".to_string());
        let repo_err: git::RepositorySetupError = setup_err.into();
        assert!(matches!(
            repo_err,
            git::RepositorySetupError::BranchExists(ref b) if b == "main"
        ));
    }

    // =========================================================================
    // Unit Tests for State Transitions
    // =========================================================================

    /// # State Transition - Handle StepStarted Message
    ///
    /// Tests that handling StepStarted updates the progress.
    #[test]
    fn test_handle_step_started_message() {
        let mut state = SetupRepoState::new();
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            step_data: StepData::default(),
        };

        state.handle_progress_message(ProgressMessage::StepStarted(WizardStep::FetchDetails));

        if let SetupState::Running { progress, .. } = &state.state {
            assert_eq!(progress.current_step, Some(WizardStep::FetchDetails));
        } else {
            panic!("Expected Running state");
        }
    }

    /// # State Transition - Handle StepCompleted Message
    ///
    /// Tests that handling StepCompleted updates progress and merges result.
    #[test]
    fn test_handle_step_completed_message() {
        let mut state = SetupRepoState::new();
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            step_data: StepData::default(),
        };

        // Start the step first
        state.handle_progress_message(ProgressMessage::StepStarted(WizardStep::FetchDetails));

        // Complete the step with result
        let result = StepResult {
            ssh_url: Some("git@example.com:repo.git".to_string()),
            ..Default::default()
        };
        state.handle_progress_message(ProgressMessage::StepCompleted(
            WizardStep::FetchDetails,
            result,
        ));

        if let SetupState::Running {
            progress,
            step_data,
        } = &state.state
        {
            assert_eq!(progress.fetch_details, StepStatus::Completed);
            assert_eq!(
                step_data.ssh_url,
                Some("git@example.com:repo.git".to_string())
            );
        } else {
            panic!("Expected Running state");
        }
    }

    /// # State Transition - Handle Error Message
    ///
    /// Tests that handling Error transitions to Error state.
    #[test]
    fn test_handle_error_message() {
        let mut state = SetupRepoState::new();
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            step_data: StepData::default(),
        };

        state.handle_progress_message(ProgressMessage::Error(SetupError::BranchExists(
            "main".to_string(),
        )));

        assert!(matches!(state.state, SetupState::Error { .. }));
    }

    /// # State Transition - Handle AllComplete Message
    ///
    /// Tests that handling AllComplete transitions to Complete state.
    #[test]
    fn test_handle_all_complete_message() {
        let mut state = SetupRepoState::new();
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            step_data: StepData {
                repo_path: Some(PathBuf::from("/tmp/repo")),
                ..Default::default()
            },
        };

        state.handle_progress_message(ProgressMessage::AllComplete);

        assert!(matches!(state.state, SetupState::Complete { .. }));
    }

    // =========================================================================
    // Integration Tests for ConfigureRepository Step
    // =========================================================================

    /// # ConfigureRepository Step - Disables Hooks When run_hooks=false
    ///
    /// Tests that the ConfigureRepository step sets core.hooksPath to /dev/null
    /// when run_hooks is false.
    ///
    /// ## Test Scenario
    /// - Creates a temporary git repository
    /// - Executes the ConfigureRepository step with run_hooks=false
    /// - Verifies that core.hooksPath is set to /dev/null
    ///
    /// ## Expected Outcome
    /// - The repository has hooks disabled via core.hooksPath=/dev/null
    #[test]
    fn test_configure_repository_disables_hooks() {
        use tempfile::TempDir;

        // Create a temporary directory and initialize a git repo
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .unwrap();

        // Create a minimal SetupContext with run_hooks=false
        let ctx = create_test_setup_context(false);

        // Execute the ConfigureRepository step
        let mut ssh_url = None;
        let mut repo_path_opt = Some(repo_path.clone());
        let mut base_repo_path = None;
        let mut is_worktree = false;
        let mut branch_name = None;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(execute_step_impl(
            WizardStep::ConfigureRepository,
            &ctx,
            &mut ssh_url,
            &mut repo_path_opt,
            &mut base_repo_path,
            &mut is_worktree,
            &mut branch_name,
        ));

        assert!(result.is_ok(), "ConfigureRepository step should succeed");

        // Verify hooks are disabled
        let output = std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "--get", "core.hooksPath"])
            .output()
            .unwrap();

        let hooks_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            hooks_path, "/dev/null",
            "Hooks should be disabled when run_hooks=false"
        );
    }

    /// # ConfigureRepository Step - Does Not Disable Hooks When run_hooks=true
    ///
    /// Tests that the ConfigureRepository step does NOT set core.hooksPath
    /// when run_hooks is true.
    ///
    /// ## Test Scenario
    /// - Creates a temporary git repository
    /// - Executes the ConfigureRepository step with run_hooks=true
    /// - Verifies that core.hooksPath is NOT set to /dev/null
    ///
    /// ## Expected Outcome
    /// - The repository does NOT have core.hooksPath set to /dev/null
    #[test]
    fn test_configure_repository_keeps_hooks_enabled() {
        use tempfile::TempDir;

        // Create a temporary directory and initialize a git repo
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .unwrap();

        // Create a minimal SetupContext with run_hooks=true
        let ctx = create_test_setup_context(true);

        // Execute the ConfigureRepository step
        let mut ssh_url = None;
        let mut repo_path_opt = Some(repo_path.clone());
        let mut base_repo_path = None;
        let mut is_worktree = false;
        let mut branch_name = None;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(execute_step_impl(
            WizardStep::ConfigureRepository,
            &ctx,
            &mut ssh_url,
            &mut repo_path_opt,
            &mut base_repo_path,
            &mut is_worktree,
            &mut branch_name,
        ));

        assert!(result.is_ok(), "ConfigureRepository step should succeed");

        // Verify hooks are NOT disabled
        let output = std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "--get", "core.hooksPath"])
            .output()
            .unwrap();

        let hooks_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(
            hooks_path.is_empty() || hooks_path != "/dev/null",
            "Hooks should NOT be disabled when run_hooks=true"
        );
    }

    /// # ConfigureRepository Step - Fails Without Repo Path
    ///
    /// Tests that the ConfigureRepository step returns an error when
    /// repo_path is not set.
    ///
    /// ## Test Scenario
    /// - Executes the ConfigureRepository step without a repo_path
    ///
    /// ## Expected Outcome
    /// - Returns SetupError::Other indicating repo path not set
    #[test]
    fn test_configure_repository_fails_without_repo_path() {
        let ctx = create_test_setup_context(false);

        let mut ssh_url = None;
        let mut repo_path_opt = None; // No repo path set
        let mut base_repo_path = None;
        let mut is_worktree = false;
        let mut branch_name = None;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(execute_step_impl(
            WizardStep::ConfigureRepository,
            &ctx,
            &mut ssh_url,
            &mut repo_path_opt,
            &mut base_repo_path,
            &mut is_worktree,
            &mut branch_name,
        ));

        assert!(result.is_err(), "Should fail without repo path");
        if let Err(SetupError::Other(msg)) = result {
            assert!(
                msg.contains("Repository path not set"),
                "Error message should indicate missing repo path"
            );
        } else {
            panic!("Expected SetupError::Other");
        }
    }
}
