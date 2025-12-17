//! Merge mode state enum for type-safe state transitions.
//!
//! This module defines the [`MergeState`] enum which represents all possible
//! states in the merge (default) mode. Using an enum instead of trait objects
//! provides compile-time type safety and eliminates virtual dispatch overhead.

use super::{
    CherryPickContinueState, CherryPickState, CompletionState, ConflictResolutionState,
    DataLoadingState, PostCompletionState, PullRequestSelectionState, SetupRepoState,
    VersionInputState,
};
use crate::ui::apps::MergeApp;
use crate::ui::state::shared::{ErrorState, SettingsConfirmationState};
use crate::ui::state::typed::{TypedAppState, TypedModeState, TypedStateChange};
use async_trait::async_trait;
use crossterm::event::{KeyCode, MouseEvent};
use ratatui::Frame;

/// All possible states for merge (default) mode.
///
/// This enum provides type-safe state management for the merge workflow.
/// Each variant wraps a concrete state type, allowing direct dispatch
/// without virtual method calls.
///
/// # States
///
/// The merge workflow progresses through these states:
/// 1. `SettingsConfirmation` - Confirm settings before starting
/// 2. `DataLoading` - Fetch PRs and work items from Azure DevOps
/// 3. `PullRequestSelection` - User selects PRs to merge
/// 4. `VersionInput` - User enters version for tagging
/// 5. `SetupRepo` - Clone or prepare the repository
/// 6. `CherryPick` - Cherry-pick selected commits
/// 7. `ConflictResolution` - Handle merge conflicts (if any)
/// 8. `CherryPickContinue` - Continue after conflict resolution
/// 9. `Completion` - Show completion summary
/// 10. `PostCompletion` - Handle post-merge tasks
/// 11. `Error` - Display error messages
///
/// # Example
///
/// ```ignore
/// let mut state = MergeState::DataLoading(DataLoadingState::new());
///
/// // Process state machine
/// match state.process_key(KeyCode::Enter, &mut app).await {
///     TypedStateChange::Keep => { /* stay in current state */ }
///     TypedStateChange::Change(new_state) => state = new_state,
///     TypedStateChange::Exit => { /* exit application */ }
/// }
/// ```
#[allow(clippy::large_enum_variant)]
pub enum MergeState {
    /// Settings confirmation screen (boxed to reduce enum size).
    SettingsConfirmation(Box<SettingsConfirmationState>),
    /// Loading data from Azure DevOps.
    DataLoading(DataLoadingState),
    /// Pull request selection screen.
    PullRequestSelection(PullRequestSelectionState),
    /// Version input for tagging.
    VersionInput(VersionInputState),
    /// Repository setup (clone/worktree).
    SetupRepo(SetupRepoState),
    /// Cherry-picking commits.
    CherryPick(CherryPickState),
    /// Conflict resolution screen.
    ConflictResolution(ConflictResolutionState),
    /// Continue cherry-pick after resolution.
    CherryPickContinue(CherryPickContinueState),
    /// Completion summary screen.
    Completion(CompletionState),
    /// Post-completion tasks screen.
    PostCompletion(PostCompletionState),
    /// Error display screen.
    Error(ErrorState),
}

impl std::fmt::Debug for MergeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MergeState::{}", self.name())
    }
}

impl MergeState {
    /// Create the initial state for merge mode.
    ///
    /// Returns `DataLoading` state to start fetching data.
    pub fn initial() -> Self {
        MergeState::DataLoading(DataLoadingState::new())
    }

    /// Create the initial state with settings confirmation.
    ///
    /// Returns `SettingsConfirmation` state if confirmation is required.
    pub fn initial_with_confirmation(config: crate::models::AppConfig) -> Self {
        MergeState::SettingsConfirmation(Box::new(SettingsConfirmationState::new(config)))
    }

    /// Get the name of the current state for logging/debugging.
    pub fn name(&self) -> &'static str {
        match self {
            MergeState::SettingsConfirmation(_) => "SettingsConfirmation",
            MergeState::DataLoading(_) => "DataLoading",
            MergeState::PullRequestSelection(_) => "PullRequestSelection",
            MergeState::VersionInput(_) => "VersionInput",
            MergeState::SetupRepo(_) => "SetupRepo",
            MergeState::CherryPick(_) => "CherryPick",
            MergeState::ConflictResolution(_) => "ConflictResolution",
            MergeState::CherryPickContinue(_) => "CherryPickContinue",
            MergeState::Completion(_) => "Completion",
            MergeState::PostCompletion(_) => "PostCompletion",
            MergeState::Error(_) => "Error",
        }
    }
}

// ============================================================================
// TypedAppState Implementation
// ============================================================================
//
// This implementation delegates to the inner state's TypedAppState implementation,
// providing fully typed state transitions without Box<dyn AppState>.

#[async_trait]
impl TypedAppState for MergeState {
    type App = MergeApp;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        match self {
            MergeState::SettingsConfirmation(state) => state.render(f),
            MergeState::DataLoading(state) => TypedModeState::ui(state, f, app),
            MergeState::PullRequestSelection(state) => TypedModeState::ui(state, f, app),
            MergeState::VersionInput(state) => TypedModeState::ui(state, f, app),
            MergeState::SetupRepo(state) => TypedModeState::ui(state, f, app),
            MergeState::CherryPick(state) => TypedModeState::ui(state, f, app),
            MergeState::CherryPickContinue(state) => TypedModeState::ui(state, f, app),
            MergeState::ConflictResolution(state) => TypedModeState::ui(state, f, app),
            MergeState::Completion(state) => TypedModeState::ui(state, f, app),
            MergeState::PostCompletion(state) => TypedModeState::ui(state, f, app),
            MergeState::Error(state) => state.render(f, app.error_message()),
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> TypedStateChange<Self> {
        match self {
            MergeState::SettingsConfirmation(state) => state.handle_key(code, |_config| {
                MergeState::DataLoading(DataLoadingState::new())
            }),
            MergeState::DataLoading(state) => TypedModeState::process_key(state, code, app).await,
            MergeState::PullRequestSelection(state) => {
                TypedModeState::process_key(state, code, app).await
            }
            MergeState::VersionInput(state) => TypedModeState::process_key(state, code, app).await,
            MergeState::SetupRepo(state) => TypedModeState::process_key(state, code, app).await,
            MergeState::CherryPick(state) => TypedModeState::process_key(state, code, app).await,
            MergeState::CherryPickContinue(state) => {
                TypedModeState::process_key(state, code, app).await
            }
            MergeState::ConflictResolution(state) => {
                TypedModeState::process_key(state, code, app).await
            }
            MergeState::Completion(state) => TypedModeState::process_key(state, code, app).await,
            MergeState::PostCompletion(state) => {
                TypedModeState::process_key(state, code, app).await
            }
            MergeState::Error(state) => state.handle_key(code),
        }
    }

    async fn process_mouse(
        &mut self,
        event: MouseEvent,
        app: &mut MergeApp,
    ) -> TypedStateChange<Self> {
        match self {
            MergeState::SettingsConfirmation(_) => TypedStateChange::Keep,
            MergeState::DataLoading(state) => {
                TypedModeState::process_mouse(state, event, app).await
            }
            MergeState::PullRequestSelection(state) => {
                TypedModeState::process_mouse(state, event, app).await
            }
            MergeState::VersionInput(state) => {
                TypedModeState::process_mouse(state, event, app).await
            }
            MergeState::SetupRepo(state) => TypedModeState::process_mouse(state, event, app).await,
            MergeState::CherryPick(state) => TypedModeState::process_mouse(state, event, app).await,
            MergeState::CherryPickContinue(state) => {
                TypedModeState::process_mouse(state, event, app).await
            }
            MergeState::ConflictResolution(state) => {
                TypedModeState::process_mouse(state, event, app).await
            }
            MergeState::Completion(state) => TypedModeState::process_mouse(state, event, app).await,
            MergeState::PostCompletion(state) => {
                TypedModeState::process_mouse(state, event, app).await
            }
            MergeState::Error(_) => TypedStateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        MergeState::name(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # MergeState Initial State
    ///
    /// Tests that the initial state is DataLoading.
    ///
    /// ## Test Scenario
    /// - Calls MergeState::initial()
    ///
    /// ## Expected Outcome
    /// - Should return DataLoading variant
    #[test]
    fn test_merge_state_initial() {
        let state = MergeState::initial();
        assert!(matches!(state, MergeState::DataLoading(_)));
        assert_eq!(state.name(), "DataLoading");
    }

    /// # MergeState Initial With Confirmation
    ///
    /// Tests that initial_with_confirmation returns SettingsConfirmation.
    ///
    /// ## Test Scenario
    /// - Creates a test config
    /// - Calls MergeState::initial_with_confirmation()
    ///
    /// ## Expected Outcome
    /// - Should return SettingsConfirmation variant
    #[test]
    fn test_merge_state_initial_with_confirmation() {
        use crate::models::{AppConfig, DefaultModeConfig, SharedConfig};
        use crate::parsed_property::ParsedProperty;

        let config = AppConfig::Default {
            shared: SharedConfig {
                organization: ParsedProperty::Default("test-org".to_string()),
                project: ParsedProperty::Default("test-project".to_string()),
                repository: ParsedProperty::Default("test-repo".to_string()),
                pat: ParsedProperty::Default("test-pat".to_string()),
                dev_branch: ParsedProperty::Default("develop".to_string()),
                target_branch: ParsedProperty::Default("main".to_string()),
                local_repo: None,
                parallel_limit: ParsedProperty::Default(4),
                max_concurrent_network: ParsedProperty::Default(10),
                max_concurrent_processing: ParsedProperty::Default(5),
                tag_prefix: ParsedProperty::Default("merged/".to_string()),
                since: None,
                skip_confirmation: false,
            },
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        };

        let state = MergeState::initial_with_confirmation(config);
        assert!(matches!(state, MergeState::SettingsConfirmation(_)));
        assert_eq!(state.name(), "SettingsConfirmation");
    }

    /// # MergeState Name For All Variants
    ///
    /// Tests that name() returns correct values for all variants.
    ///
    /// ## Test Scenario
    /// - Creates each MergeState variant
    /// - Checks the name
    ///
    /// ## Expected Outcome
    /// - Each variant should have a unique, correct name
    #[test]
    fn test_merge_state_names() {
        // Test a few representative variants
        let data_loading = MergeState::DataLoading(DataLoadingState::new());
        assert_eq!(data_loading.name(), "DataLoading");

        let pr_selection = MergeState::PullRequestSelection(PullRequestSelectionState::new());
        assert_eq!(pr_selection.name(), "PullRequestSelection");

        let error = MergeState::Error(ErrorState::new());
        assert_eq!(error.name(), "Error");
    }

    /// # MergeState Debug Implementation
    ///
    /// Tests that MergeState implements Debug correctly.
    ///
    /// ## Test Scenario
    /// - Creates a MergeState
    /// - Formats it using Debug
    ///
    /// ## Expected Outcome
    /// - Should produce readable debug output
    #[test]
    fn test_merge_state_debug() {
        let state = MergeState::initial();
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("DataLoading"));
    }
}
