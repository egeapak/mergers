//! Cleanup mode state enum for type-safe state transitions.
//!
//! This module defines the [`CleanupModeState`] enum which represents all possible
//! states in the cleanup mode. Using an enum instead of trait objects
//! provides compile-time type safety and eliminates virtual dispatch overhead.

use super::{
    CleanupBranchSelectionState, CleanupDataLoadingState, CleanupExecutionState,
    CleanupResultsState,
};
use crate::ui::apps::CleanupApp;
use crate::ui::state::shared::{ErrorState, SettingsConfirmationState};
use crate::ui::state::typed::{TypedAppState, TypedStateChange};
use async_trait::async_trait;
use crossterm::event::{KeyCode, MouseEvent};
use ratatui::Frame;

/// All possible states for cleanup mode.
///
/// This enum provides type-safe state management for the cleanup workflow.
/// Each variant wraps a concrete state type, allowing direct dispatch
/// without virtual method calls.
///
/// # States
///
/// The cleanup workflow progresses through these states:
/// 1. `SettingsConfirmation` - Confirm settings before starting
/// 2. `DataLoading` - Fetch branch information and determine cleanup candidates
/// 3. `BranchSelection` - User selects branches to clean up
/// 4. `Execution` - Delete selected branches
/// 5. `Results` - Display cleanup results
/// 6. `Error` - Display error messages
///
/// # Example
///
/// ```ignore
/// let mut state = CleanupModeState::DataLoading(
///     CleanupDataLoadingState::new(config)
/// );
///
/// // Process state machine
/// match state.process_key(KeyCode::Enter, &mut app).await {
///     TypedStateChange::Keep => { /* stay in current state */ }
///     TypedStateChange::Change(new_state) => state = new_state,
///     TypedStateChange::Exit => { /* exit application */ }
/// }
/// ```
#[allow(clippy::large_enum_variant)]
pub enum CleanupModeState {
    /// Settings confirmation screen (boxed to reduce enum size).
    SettingsConfirmation(Box<SettingsConfirmationState>),
    /// Loading branch data.
    DataLoading(CleanupDataLoadingState),
    /// Branch selection screen.
    BranchSelection(CleanupBranchSelectionState),
    /// Executing cleanup operations.
    Execution(CleanupExecutionState),
    /// Cleanup results display.
    Results(CleanupResultsState),
    /// Error display screen.
    Error(ErrorState),
}

impl std::fmt::Debug for CleanupModeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CleanupModeState::{}", self.name())
    }
}

impl CleanupModeState {
    /// Create the initial state for cleanup mode with data loading.
    ///
    /// # Arguments
    ///
    /// * `config` - The application configuration
    ///
    /// # Returns
    ///
    /// A `DataLoading` state to start scanning for cleanup branches.
    pub fn initial(config: crate::models::AppConfig) -> Self {
        CleanupModeState::DataLoading(CleanupDataLoadingState::new(config))
    }

    /// Create the initial state with settings confirmation.
    ///
    /// Returns `SettingsConfirmation` state if confirmation is required.
    pub fn initial_with_confirmation(config: crate::models::AppConfig) -> Self {
        CleanupModeState::SettingsConfirmation(Box::new(SettingsConfirmationState::new(config)))
    }

    /// Get the name of the current state for logging/debugging.
    pub fn name(&self) -> &'static str {
        match self {
            CleanupModeState::SettingsConfirmation(_) => "SettingsConfirmation",
            CleanupModeState::DataLoading(_) => "DataLoading",
            CleanupModeState::BranchSelection(_) => "BranchSelection",
            CleanupModeState::Execution(_) => "Execution",
            CleanupModeState::Results(_) => "Results",
            CleanupModeState::Error(_) => "Error",
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
impl TypedAppState for CleanupModeState {
    type App = CleanupApp;
    type StateEnum = CleanupModeState;

    fn ui(&mut self, f: &mut Frame, app: &CleanupApp) {
        match self {
            CleanupModeState::SettingsConfirmation(state) => state.render(f),
            CleanupModeState::DataLoading(state) => TypedAppState::ui(state, f, app),
            CleanupModeState::BranchSelection(state) => TypedAppState::ui(state, f, app),
            CleanupModeState::Execution(state) => TypedAppState::ui(state, f, app),
            CleanupModeState::Results(state) => TypedAppState::ui(state, f, app),
            CleanupModeState::Error(state) => state.render(f, app.error_message()),
        }
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut CleanupApp,
    ) -> TypedStateChange<CleanupModeState> {
        match self {
            CleanupModeState::SettingsConfirmation(state) => state.handle_key(code, |config| {
                CleanupModeState::DataLoading(CleanupDataLoadingState::new(config.clone()))
            }),
            CleanupModeState::DataLoading(state) => {
                TypedAppState::process_key(state, code, app).await
            }
            CleanupModeState::BranchSelection(state) => {
                TypedAppState::process_key(state, code, app).await
            }
            CleanupModeState::Execution(state) => {
                TypedAppState::process_key(state, code, app).await
            }
            CleanupModeState::Results(state) => TypedAppState::process_key(state, code, app).await,
            CleanupModeState::Error(state) => state.handle_key(code),
        }
    }

    async fn process_mouse(
        &mut self,
        event: MouseEvent,
        app: &mut CleanupApp,
    ) -> TypedStateChange<CleanupModeState> {
        match self {
            CleanupModeState::SettingsConfirmation(_) => TypedStateChange::Keep,
            CleanupModeState::DataLoading(state) => {
                TypedAppState::process_mouse(state, event, app).await
            }
            CleanupModeState::BranchSelection(state) => {
                TypedAppState::process_mouse(state, event, app).await
            }
            CleanupModeState::Execution(state) => {
                TypedAppState::process_mouse(state, event, app).await
            }
            CleanupModeState::Results(state) => {
                TypedAppState::process_mouse(state, event, app).await
            }
            CleanupModeState::Error(_) => TypedStateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        CleanupModeState::name(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppConfig, CleanupModeConfig, SharedConfig};
    use crate::parsed_property::ParsedProperty;

    fn create_test_cleanup_config() -> AppConfig {
        AppConfig::Cleanup {
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
            cleanup: CleanupModeConfig {
                target: ParsedProperty::Default("main".to_string()),
            },
        }
    }

    /// # CleanupModeState Initial State
    ///
    /// Tests that the initial state is DataLoading.
    ///
    /// ## Test Scenario
    /// - Calls CleanupModeState::initial()
    ///
    /// ## Expected Outcome
    /// - Should return DataLoading variant
    #[test]
    fn test_cleanup_mode_state_initial() {
        let config = create_test_cleanup_config();
        let state = CleanupModeState::initial(config);
        assert!(matches!(state, CleanupModeState::DataLoading(_)));
        assert_eq!(state.name(), "DataLoading");
    }

    /// # CleanupModeState Initial With Confirmation
    ///
    /// Tests that initial_with_confirmation returns SettingsConfirmation.
    ///
    /// ## Test Scenario
    /// - Creates a test config
    /// - Calls CleanupModeState::initial_with_confirmation()
    ///
    /// ## Expected Outcome
    /// - Should return SettingsConfirmation variant
    #[test]
    fn test_cleanup_mode_state_initial_with_confirmation() {
        let config = create_test_cleanup_config();
        let state = CleanupModeState::initial_with_confirmation(config);
        assert!(matches!(state, CleanupModeState::SettingsConfirmation(_)));
        assert_eq!(state.name(), "SettingsConfirmation");
    }

    /// # CleanupModeState Name For All Variants
    ///
    /// Tests that name() returns correct values for variants.
    ///
    /// ## Test Scenario
    /// - Creates various CleanupModeState variants
    /// - Checks the names
    ///
    /// ## Expected Outcome
    /// - Each variant should have a unique, correct name
    #[test]
    fn test_cleanup_mode_state_names() {
        let config = create_test_cleanup_config();

        let data_loading = CleanupModeState::DataLoading(CleanupDataLoadingState::new(config));
        assert_eq!(data_loading.name(), "DataLoading");

        let error = CleanupModeState::Error(ErrorState::new());
        assert_eq!(error.name(), "Error");
    }

    /// # CleanupModeState Debug Implementation
    ///
    /// Tests that CleanupModeState implements Debug correctly.
    ///
    /// ## Test Scenario
    /// - Creates a CleanupModeState
    /// - Formats it using Debug
    ///
    /// ## Expected Outcome
    /// - Should produce readable debug output
    #[test]
    fn test_cleanup_mode_state_debug() {
        let config = create_test_cleanup_config();
        let state = CleanupModeState::initial(config);
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("DataLoading"));
    }
}
