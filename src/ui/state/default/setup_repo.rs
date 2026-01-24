// Allow deprecated RepositorySetupError usage until full migration to GitError
#![allow(deprecated)]

use super::MergeState;
use crate::{
    core::state::MergePhase,
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

/// Intermediate data stored between wizard steps
#[derive(Debug, Clone, Default)]
pub struct StepData {
    /// SSH URL fetched from Azure DevOps (clone mode)
    pub ssh_url: Option<String>,
    /// Repository path after clone/worktree creation
    pub repo_path: Option<std::path::PathBuf>,
    /// Whether this is a worktree (vs clone)
    pub is_worktree: bool,
    /// Base repo path for worktree cleanup
    pub base_repo_path: Option<std::path::PathBuf>,
    /// Branch name to create
    pub branch_name: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SetupState {
    Initializing,
    InProgress {
        progress: WizardProgress,
        /// The next step to execute (None if waiting for first tick)
        next_step: Option<WizardStep>,
        /// Intermediate data stored between steps
        step_data: StepData,
    },
    Error {
        error: git::RepositorySetupError,
        message: String,
        /// Progress at the time of error (to show which step failed)
        progress: Option<WizardProgress>,
    },
}

pub struct SetupRepoState {
    state: SetupState,
    started: bool,
    /// Cached mode detection (None until first run)
    is_clone_mode: Option<bool>,
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
            started: false,
            is_clone_mode: None,
        }
    }

    /// Initialize the wizard progress tracker and set the first step
    fn init_progress(&mut self, is_clone_mode: bool) {
        self.is_clone_mode = Some(is_clone_mode);
        let first_step = if is_clone_mode {
            WizardStep::FetchDetails
        } else {
            WizardStep::CheckPrerequisites
        };
        self.state = SetupState::InProgress {
            progress: WizardProgress::new(is_clone_mode),
            next_step: Some(first_step),
            step_data: StepData::default(),
        };
    }

    /// Get mutable reference to progress if in progress state
    fn progress_mut(&mut self) -> Option<&mut WizardProgress> {
        match &mut self.state {
            SetupState::InProgress { progress, .. } => Some(progress),
            _ => None,
        }
    }

    /// Get mutable reference to step data if in progress state
    fn step_data_mut(&mut self) -> Option<&mut StepData> {
        match &mut self.state {
            SetupState::InProgress { step_data, .. } => Some(step_data),
            _ => None,
        }
    }

    /// Set the next step to execute
    fn set_next_step(&mut self, step: Option<WizardStep>) {
        if let SetupState::InProgress { next_step, .. } = &mut self.state {
            *next_step = step;
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

    fn set_error(&mut self, error: git::RepositorySetupError) {
        // Preserve the current progress to show which step failed
        let current_progress = match &self.state {
            SetupState::InProgress { progress, .. } => Some(progress.clone()),
            _ => None,
        };

        let message = match &error {
            git::RepositorySetupError::BranchExists(branch) => {
                format!(
                    "Branch '{}' already exists.\n\nThis can happen if you've run this tool before or if the branch was created elsewhere.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force delete the branch and continue\n  • Press 'Esc' to go back",
                    branch
                )
            }
            git::RepositorySetupError::WorktreeExists(path) => {
                format!(
                    "Worktree already exists at:\n{}\n\nThis can happen if you've run this tool before or if the worktree was created elsewhere.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force remove the worktree and continue\n  • Press 'Esc' to go back",
                    path
                )
            }
            git::RepositorySetupError::Other(msg) => {
                format!(
                    "Setup failed: {}\n\nOptions:\n  • Press 'r' to retry\n  • Press 'Esc' to go back",
                    msg
                )
            }
        };
        self.state = SetupState::Error {
            error: error.clone(),
            message,
            progress: current_progress,
        };
    }

    /// Execute a single wizard step and return the next step to execute.
    /// Returns None when all steps are complete (transition to CherryPick state).
    async fn execute_step(
        &mut self,
        step: WizardStep,
        app: &mut MergeApp,
    ) -> Result<Option<WizardStep>, StateChange<MergeState>> {
        self.start_step(step);

        match step {
            WizardStep::FetchDetails => {
                // Clone mode: fetch SSH URL from Azure DevOps
                match app.client().fetch_repo_details().await {
                    Ok(details) => {
                        if let Some(step_data) = self.step_data_mut() {
                            step_data.ssh_url = Some(details.ssh_url);
                        }
                        self.complete_step(step);
                        Ok(Some(WizardStep::CheckPrerequisites))
                    }
                    Err(e) => {
                        app.set_error_message(Some(format!(
                            "Failed to fetch repository details: {}",
                            e
                        )));
                        Err(StateChange::Change(MergeState::Error(ErrorState::new())))
                    }
                }
            }

            WizardStep::CheckPrerequisites => {
                // Verify the local repo exists and is valid (worktree mode)
                // For clone mode, just validate we have the SSH URL
                let is_clone_mode = self.is_clone_mode.unwrap_or(false);

                if is_clone_mode {
                    // For clone mode, verify we have the SSH URL
                    let has_url = match &self.state {
                        SetupState::InProgress { step_data, .. } => step_data.ssh_url.is_some(),
                        _ => false,
                    };
                    if !has_url {
                        self.set_error(git::RepositorySetupError::Other(
                            "SSH URL not available".to_string(),
                        ));
                        return Ok(None);
                    }
                } else {
                    // For worktree mode, verify local repo
                    if let Some(repo_path) = app.local_repo() {
                        let path = std::path::Path::new(repo_path);
                        if !path.exists() {
                            self.set_error(git::RepositorySetupError::Other(format!(
                                "Local repository path does not exist: {:?}",
                                path
                            )));
                            return Ok(None);
                        }
                    }
                }

                self.complete_step(step);
                if is_clone_mode {
                    Ok(Some(WizardStep::CloneOrWorktree))
                } else {
                    Ok(Some(WizardStep::FetchTargetBranch))
                }
            }

            WizardStep::FetchTargetBranch => {
                // Worktree mode: fetch target branch from remote
                let target_branch = app.target_branch().to_string();
                if let Some(repo_path) = app.local_repo() {
                    let output = std::process::Command::new("git")
                        .current_dir(repo_path)
                        .args(["fetch", "origin", &target_branch])
                        .output();

                    match output {
                        Ok(result) if result.status.success() => {
                            self.complete_step(step);
                            Ok(Some(WizardStep::CloneOrWorktree))
                        }
                        Ok(result) => {
                            self.set_error(git::RepositorySetupError::Other(format!(
                                "Failed to fetch target branch: {}",
                                String::from_utf8_lossy(&result.stderr)
                            )));
                            Ok(None)
                        }
                        Err(e) => {
                            self.set_error(git::RepositorySetupError::Other(format!(
                                "Failed to fetch target branch: {}",
                                e
                            )));
                            Ok(None)
                        }
                    }
                } else {
                    self.set_error(git::RepositorySetupError::Other(
                        "Local repository path not set".to_string(),
                    ));
                    Ok(None)
                }
            }

            WizardStep::CloneOrWorktree => {
                let is_clone_mode = self.is_clone_mode.unwrap_or(false);
                let version = app.version().as_ref().unwrap().to_string();
                let target_branch = app.target_branch().to_string();
                let run_hooks = app.run_hooks();

                if is_clone_mode {
                    // Clone mode
                    let ssh_url = match &self.state {
                        SetupState::InProgress { step_data, .. } => {
                            step_data.ssh_url.clone().unwrap_or_default()
                        }
                        _ => String::new(),
                    };

                    match git::shallow_clone_repo(&ssh_url, &target_branch, run_hooks) {
                        Ok((path, temp_dir)) => {
                            app.set_repo_path(Some(path.clone()));
                            app.worktree.set_temp_dir(Some(temp_dir));
                            if let Some(step_data) = self.step_data_mut() {
                                step_data.repo_path = Some(path);
                                step_data.is_worktree = false;
                            }
                            self.complete_step(step);
                            // Skip ConfigureRepository for clone mode (already configured)
                            Ok(Some(WizardStep::CreateBranch))
                        }
                        Err(e) => {
                            self.set_error(git::RepositorySetupError::Other(e.to_string()));
                            Ok(None)
                        }
                    }
                } else {
                    // Worktree mode
                    let local_repo = app.local_repo().map(std::path::PathBuf::from);
                    if let Some(base_path) = &local_repo {
                        match git::create_worktree(base_path, &target_branch, &version, run_hooks) {
                            Ok(worktree_path) => {
                                app.worktree.base_repo_path = Some(base_path.clone());
                                app.set_repo_path(Some(worktree_path.clone()));
                                if let Some(step_data) = self.step_data_mut() {
                                    step_data.repo_path = Some(worktree_path);
                                    step_data.is_worktree = true;
                                    step_data.base_repo_path = Some(base_path.clone());
                                }
                                self.complete_step(step);
                                // Skip ConfigureRepository (already done in create_worktree)
                                Ok(Some(WizardStep::CreateBranch))
                            }
                            Err(e) => {
                                self.set_error(e);
                                Ok(None)
                            }
                        }
                    } else {
                        self.set_error(git::RepositorySetupError::Other(
                            "Local repository path not set".to_string(),
                        ));
                        Ok(None)
                    }
                }
            }

            WizardStep::ConfigureRepository => {
                // This step is currently handled within clone/worktree creation
                // but we keep it for visual feedback
                self.complete_step(step);
                Ok(Some(WizardStep::CreateBranch))
            }

            WizardStep::CreateBranch => {
                let version = app.version().as_ref().unwrap().to_string();
                let target_branch = app.target_branch().to_string();
                let branch_name = format!("patch/{}-{}", target_branch, version);

                if let Some(repo_path) = app.repo_path() {
                    match git::create_branch(repo_path, &branch_name) {
                        Ok(()) => {
                            if let Some(step_data) = self.step_data_mut() {
                                step_data.branch_name = Some(branch_name);
                            }
                            self.complete_step(step);
                            Ok(Some(WizardStep::PrepareCherryPicks))
                        }
                        Err(e) => {
                            // Check if it's a branch exists error
                            let err_msg = e.to_string();
                            if err_msg.contains("already exists") {
                                self.set_error(git::RepositorySetupError::BranchExists(
                                    branch_name,
                                ));
                            } else {
                                app.set_error_message(Some(format!(
                                    "Failed to create branch: {}",
                                    e
                                )));
                                return Err(StateChange::Change(MergeState::Error(
                                    ErrorState::new(),
                                )));
                            }
                            Ok(None)
                        }
                    }
                } else {
                    self.set_error(git::RepositorySetupError::Other(
                        "Repository path not set".to_string(),
                    ));
                    Ok(None)
                }
            }

            WizardStep::PrepareCherryPicks => {
                // Prepare cherry-pick items from selected PRs
                let selected_prs = app.get_selected_prs();
                let cherry_pick_items: Vec<CherryPickItem> = selected_prs
                    .iter()
                    .filter_map(|pr| {
                        pr.pr
                            .last_merge_commit
                            .as_ref()
                            .map(|commit| CherryPickItem {
                                commit_id: commit.commit_id.clone(),
                                pr_id: pr.pr.id,
                                pr_title: pr.pr.title.clone(),
                                status: crate::models::CherryPickStatus::Pending,
                            })
                    })
                    .collect();

                if cherry_pick_items.is_empty() {
                    app.set_error_message(Some("No commits found to cherry-pick".to_string()));
                    return Err(StateChange::Change(MergeState::Error(ErrorState::new())));
                }

                *app.cherry_pick_items_mut() = cherry_pick_items;
                self.complete_step(step);
                Ok(Some(WizardStep::InitializeState))
            }

            WizardStep::InitializeState => {
                // Create state file for cross-mode resume support
                if let Some(repo_path) = app.repo_path() {
                    let version = app.version().as_ref().unwrap().to_string();
                    let base_repo_path = app.worktree.base_repo_path.clone();
                    let is_worktree = base_repo_path.is_some();

                    // State file creation is optional for TUI - silently ignore errors
                    if app
                        .create_state_file(
                            repo_path.to_path_buf(),
                            base_repo_path,
                            is_worktree,
                            &version,
                        )
                        .is_ok()
                    {
                        // Set initial phase to CherryPicking
                        let _ = app.update_state_phase(MergePhase::CherryPicking);
                    }
                }

                self.complete_step(step);
                // All steps complete - transition to CherryPick state
                Ok(None)
            }
        }
    }

    /// Initialize the wizard and prepare for step-by-step execution
    fn setup_repository_init(&mut self, app: &MergeApp) {
        let is_clone_mode = app.local_repo().is_none();
        self.init_progress(is_clone_mode);
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
                    app.set_error_message(Some(format!(
                        "Failed to force delete branch: {}",
                        e
                    )));
                    return StateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
            git::RepositorySetupError::WorktreeExists(_) => {
                // Force remove the worktree before retrying
                if let Some(repo_path) = app.local_repo()
                    && let Err(e) =
                        git::force_remove_worktree(std::path::Path::new(repo_path), version)
                {
                    app.set_error_message(Some(format!(
                        "Failed to force remove worktree: {}",
                        e
                    )));
                    return StateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
            git::RepositorySetupError::Other(_) => {
                // For other errors, just retry
            }
        }

        // After force operation, reset and restart the setup
        self.state = SetupState::Initializing;
        self.started = false;
        StateChange::Keep
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
            SetupState::InProgress { progress, .. } => {
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
        match &self.state {
            SetupState::Error { error, .. } => {
                match code {
                    KeyCode::Char('r' | 'R') => {
                        // Retry - reset state and try again
                        self.state = SetupState::Initializing;
                        self.started = false;
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
                // Initialize the wizard on first tick
                if !self.started {
                    self.started = true;
                    self.setup_repository_init(app);
                }
                StateChange::Keep
            }
            SetupState::InProgress { next_step, .. } => {
                // Execute the next step on each tick (KeyCode::Null or any key)
                if let Some(step) = *next_step {
                    match self.execute_step(step, app).await {
                        Ok(Some(next)) => {
                            // Set the next step and keep state for re-render
                            self.set_next_step(Some(next));
                            StateChange::Keep
                        }
                        Ok(None) => {
                            // All steps complete - transition to CherryPick
                            StateChange::Change(MergeState::CherryPick(CherryPickState::new()))
                        }
                        Err(state_change) => state_change,
                    }
                } else {
                    StateChange::Keep
                }
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

    /// Helper to create an InProgress state with given progress
    fn make_in_progress(progress: WizardProgress) -> SetupState {
        SetupState::InProgress {
            progress,
            next_step: None,
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
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
            inner_state.state = make_in_progress(progress);
            inner_state.is_clone_mode = Some(true);

            // Now trigger an error (this preserves the progress)
            inner_state.set_error(git::RepositorySetupError::WorktreeExists(
                "/path/to/repo/.worktrees/v1.0.0".to_string(),
            ));

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
            inner_state.set_error(git::RepositorySetupError::BranchExists(
                "patch/main-v1.0.0".to_string(),
            ));
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
            inner_state.set_error(git::RepositorySetupError::WorktreeExists(
                "/path/to/repo/.worktrees/v1.0.0".to_string(),
            ));
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
            inner_state.set_error(git::RepositorySetupError::Other(
                "Failed to fetch repository details from Azure DevOps".to_string(),
            ));
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
    /// - Should initialize with Initializing state and started=false
    #[test]
    fn test_setup_repo_default() {
        let state = SetupRepoState::default();
        assert!(!state.started);
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
        state.set_error(git::RepositorySetupError::Other("Test error".to_string()));

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
        state.set_error(git::RepositorySetupError::Other("Test error".to_string()));

        for key in [KeyCode::Up, KeyCode::Down, KeyCode::Char('x')] {
            let result = ModeState::process_key(&mut state, key, harness.merge_app_mut()).await;
            assert!(matches!(result, StateChange::Keep));
        }
    }

    /// # Setup Repo State - Key in Normal State When Started
    ///
    /// Tests key handling when setup has already started.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets started=true
    /// - Processes a key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep (already started)
    #[tokio::test]
    async fn test_setup_repo_key_when_started() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut state = SetupRepoState::new();
        state.started = true;

        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }
}
