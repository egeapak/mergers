// Allow deprecated RepositorySetupError usage until full migration to GitError
#![allow(deprecated)]

use super::MergeState;
use crate::{
    git,
    models::CherryPickItem,
    ui::App,
    ui::apps::MergeApp,
    ui::state::typed::{TypedAppState, TypedStateChange},
    ui::state::{AppState, CherryPickState, ErrorState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[derive(Debug, Clone)]
pub enum SetupState {
    Initializing,
    InProgress(String),
    Error {
        error: git::RepositorySetupError,
        message: String,
    },
}

pub struct SetupRepoState {
    state: SetupState,
    started: bool,
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
        }
    }

    fn set_status(&mut self, status: String) {
        self.state = SetupState::InProgress(status);
    }

    fn set_error(&mut self, error: git::RepositorySetupError) {
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
        };
    }

    async fn setup_repository(&mut self, app: &mut MergeApp) -> TypedStateChange<MergeState> {
        // Get SSH URL if needed
        let ssh_url = if app.local_repo().is_none() {
            self.set_status("Fetching repository details...".to_string());
            match app.client().fetch_repo_details().await {
                Ok(details) => details.ssh_url,
                Err(e) => {
                    app.set_error_message(Some(format!(
                        "Failed to fetch repository details: {}",
                        e
                    )));
                    return TypedStateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
        } else {
            String::new()
        };

        // Get status message before any setup operations
        let status_msg = if app.local_repo().is_some() {
            "Creating worktree...".to_string()
        } else {
            "Cloning repository...".to_string()
        };
        self.set_status(status_msg);

        // Extract ALL immutable data before any mutations
        let version: String;
        let target_branch: String;
        let local_repo_path: Option<std::path::PathBuf>;
        let setup_result: Result<git::RepositorySetup, git::RepositorySetupError>;
        let cherry_pick_items_data: Vec<(String, i32, String)>; // (commit_id, pr_id, pr_title)

        {
            version = app.version().as_ref().unwrap().to_string();
            target_branch = app.target_branch().to_string();
            local_repo_path = app.local_repo().map(std::path::PathBuf::from);
            let local_repo = app.local_repo();
            setup_result = git::setup_repository(local_repo, &ssh_url, &target_branch, &version);

            // Extract cherry-pick data
            let selected_prs = app.get_selected_prs();
            cherry_pick_items_data = selected_prs
                .iter()
                .filter_map(|pr| {
                    pr.pr
                        .last_merge_commit
                        .as_ref()
                        .map(|commit| (commit.commit_id.clone(), pr.pr.id, pr.pr.title.clone()))
                })
                .collect();
        }

        // Now handle the result with mutable access to app
        match setup_result {
            Ok(setup) => {
                match setup {
                    git::RepositorySetup::Local(path) => {
                        // Store the base repo path for cleanup (worktree case)
                        if let Some(local_repo) = local_repo_path {
                            app.worktree.base_repo_path = Some(local_repo);
                        }
                        app.set_repo_path(Some(path));
                    }
                    git::RepositorySetup::Clone(path, temp_dir) => {
                        app.set_repo_path(Some(path));
                        app.worktree.set_temp_dir(Some(temp_dir));
                        // base_repo_path stays None for cloned repos
                    }
                }

                // Prepare cherry-pick items
                let cherry_pick_items: Vec<CherryPickItem> = cherry_pick_items_data
                    .into_iter()
                    .map(|(commit_id, pr_id, pr_title)| CherryPickItem {
                        commit_id,
                        pr_id,
                        pr_title,
                        status: crate::models::CherryPickStatus::Pending,
                    })
                    .collect();

                if cherry_pick_items.is_empty() {
                    app.set_error_message(Some("No commits found to cherry-pick".to_string()));
                    TypedStateChange::Change(MergeState::Error(ErrorState::new()))
                } else {
                    *app.cherry_pick_items_mut() = cherry_pick_items;

                    // Create branch for cherry-picking
                    self.set_status("Creating branch...".to_string());
                    let branch_name = format!("patch/{}-{}", target_branch, version);

                    if let Err(e) =
                        git::create_branch(app.repo_path().as_ref().unwrap(), &branch_name)
                    {
                        app.set_error_message(Some(format!("Failed to create branch: {}", e)));
                        TypedStateChange::Change(MergeState::Error(ErrorState::new()))
                    } else {
                        TypedStateChange::Change(MergeState::CherryPick(CherryPickState::new()))
                    }
                }
            }
            Err(e) => {
                self.set_error(e);
                TypedStateChange::Keep
            }
        }
    }

    async fn force_resolve_error(
        &mut self,
        app: &mut MergeApp,
        error: git::RepositorySetupError,
    ) -> TypedStateChange<MergeState> {
        let version = app.version().unwrap();

        match error {
            git::RepositorySetupError::BranchExists(branch_name) => {
                self.set_status("Force deleting branch...".to_string());
                if let Some(repo_path) = app.local_repo()
                    && let Err(e) =
                        git::force_delete_branch(std::path::Path::new(repo_path), &branch_name)
                {
                    app.set_error_message(Some(format!("Failed to force delete branch: {}", e)));
                    return TypedStateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
            git::RepositorySetupError::WorktreeExists(_) => {
                self.set_status("Force removing worktree...".to_string());
                if let Some(repo_path) = app.local_repo()
                    && let Err(e) =
                        git::force_remove_worktree(std::path::Path::new(repo_path), version)
                {
                    app.set_error_message(Some(format!("Failed to force remove worktree: {}", e)));
                    return TypedStateChange::Change(MergeState::Error(ErrorState::new()));
                }
            }
            git::RepositorySetupError::Other(_) => {
                // For other errors, just retry
            }
        }

        // After force operation, retry the setup
        self.setup_repository(app).await
    }
}

// ============================================================================
// TypedAppState Implementation (Primary)
// ============================================================================

#[async_trait]
impl TypedAppState for SetupRepoState {
    type App = MergeApp;
    type StateEnum = MergeState;

    fn ui(&mut self, f: &mut Frame, _app: &MergeApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Min(0)])
            .split(f.area());

        match &self.state {
            SetupState::Initializing => {
                let status = Paragraph::new("Initializing repository...")
                    .style(Style::default().fg(Color::Yellow))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Repository Setup"),
                    )
                    .alignment(Alignment::Center);

                f.render_widget(status, chunks[0]);
            }
            SetupState::InProgress(message) => {
                let status = Paragraph::new(message.as_str())
                    .style(Style::default().fg(Color::Yellow))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Repository Setup"),
                    )
                    .alignment(Alignment::Center);

                f.render_widget(status, chunks[0]);
            }
            SetupState::Error { message, .. } => {
                let message_lines: Vec<Line> = message
                    .lines()
                    .map(|line| {
                        if line.starts_with("Options:") {
                            Line::from(vec![Span::styled(line, Style::default().fg(Color::Cyan))])
                        } else if line.starts_with("  •") {
                            Line::from(vec![Span::styled(line, Style::default().fg(Color::Yellow))])
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
                            .title("Repository Setup Error")
                            .title_style(Style::default().fg(Color::Red)),
                    )
                    .wrap(Wrap { trim: true })
                    .alignment(Alignment::Left);

                f.render_widget(error_paragraph, chunks[0]);
            }
        }
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MergeApp,
    ) -> TypedStateChange<MergeState> {
        match &self.state {
            SetupState::Error { error, .. } => {
                match code {
                    KeyCode::Char('r' | 'R') => {
                        // Retry - reset state and try again
                        self.state = SetupState::Initializing;
                        self.started = false;
                        self.setup_repository(app).await
                    }
                    KeyCode::Char('f' | 'F') => {
                        // Force - try to resolve the specific error and retry
                        let error_clone = error.clone();
                        self.force_resolve_error(app, error_clone).await
                    }
                    KeyCode::Esc => {
                        // Go back to previous state or exit
                        TypedStateChange::Change(MergeState::Error(ErrorState::new()))
                    }
                    _ => TypedStateChange::Keep,
                }
            }
            _ => {
                if !self.started {
                    self.started = true;
                    self.setup_repository(app).await
                } else {
                    TypedStateChange::Keep
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "SetupRepo"
    }
}

// ============================================================================
// Legacy AppState Implementation (delegates to TypedAppState)
// ============================================================================

#[async_trait]
impl AppState for SetupRepoState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        if let App::Merge(merge_app) = app {
            TypedAppState::ui(self, f, merge_app);
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        if let App::Merge(merge_app) = app {
            match <Self as TypedAppState>::process_key(self, code, merge_app).await {
                TypedStateChange::Keep => StateChange::Keep,
                TypedStateChange::Exit => StateChange::Exit,
                TypedStateChange::Change(new_state) => StateChange::Change(Box::new(new_state)),
            }
        } else {
            StateChange::Keep
        }
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

            let state = Box::new(SetupRepoState::new());
            harness.render_state(state);

            assert_snapshot!("initializing", harness.backend());
        });
    }

    /// # Setup Repo State - Cloning
    ///
    /// Tests the repository setup screen during cloning.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets state to InProgress with cloning message
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display "Cloning repository..." message
    /// - Should maintain consistent layout
    #[test]
    fn test_setup_repo_cloning() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = SetupRepoState::new();
            state.state = SetupState::InProgress("Cloning repository...".to_string());
            harness.render_state(Box::new(state));

            assert_snapshot!("cloning", harness.backend());
        });
    }

    /// # Setup Repo State - Creating Worktree
    ///
    /// Tests the repository setup screen during worktree creation.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets state to InProgress with worktree message
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display "Creating worktree..." message
    #[test]
    fn test_setup_repo_creating_worktree() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = SetupRepoState::new();
            state.state = SetupState::InProgress("Creating worktree...".to_string());
            harness.render_state(Box::new(state));

            assert_snapshot!("creating_worktree", harness.backend());
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

            let mut state = SetupRepoState::new();
            state.set_error(git::RepositorySetupError::BranchExists(
                "patch/main-v1.0.0".to_string(),
            ));
            harness.render_state(Box::new(state));

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

            let mut state = SetupRepoState::new();
            state.set_error(git::RepositorySetupError::WorktreeExists(
                "/path/to/repo/.worktrees/v1.0.0".to_string(),
            ));
            harness.render_state(Box::new(state));

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

            let mut state = SetupRepoState::new();
            state.set_error(git::RepositorySetupError::Other(
                "Failed to fetch repository details from Azure DevOps".to_string(),
            ));
            harness.render_state(Box::new(state));

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

        let result = AppState::process_key(&mut state, KeyCode::Esc, &mut harness.app).await;
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
            let result = AppState::process_key(&mut state, key, &mut harness.app).await;
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

        let result = AppState::process_key(&mut state, KeyCode::Enter, &mut harness.app).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # Setup Repo State - Creating Branch Status
    ///
    /// Tests the creating branch status message.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets status to "Creating branch..."
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display "Creating branch..." message
    #[test]
    fn test_setup_repo_creating_branch() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = SetupRepoState::new();
            state.set_status("Creating branch...".to_string());
            harness.render_state(Box::new(state));

            assert_snapshot!("creating_branch", harness.backend());
        });
    }

    /// # Setup Repo State - Fetching Repo Details Status
    ///
    /// Tests the fetching repository details status message.
    ///
    /// ## Test Scenario
    /// - Creates a setup repo state
    /// - Sets status to "Fetching repository details..."
    /// - Renders the state
    ///
    /// ## Expected Outcome
    /// - Should display "Fetching repository details..." message
    #[test]
    fn test_setup_repo_fetching_details() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = SetupRepoState::new();
            state.set_status("Fetching repository details...".to_string());
            harness.render_state(Box::new(state));

            assert_snapshot!("fetching_details", harness.backend());
        });
    }
}
