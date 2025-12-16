//! Migration mode state enum for type-safe state transitions.
//!
//! This module defines the [`MigrationModeState`] enum which represents all possible
//! states in the migration mode. Using an enum instead of trait objects
//! provides compile-time type safety and eliminates virtual dispatch overhead.
//!
//! Note: Named `MigrationModeState` to avoid conflict with the existing
//! `MigrationState` struct (re-exported as `MigrationResultsState`).

use super::{
    MigrationDataLoadingState, MigrationResultsState, MigrationTaggingState,
    MigrationVersionInputState,
};
use crate::ui::apps::MigrationApp;
use crate::ui::state::shared::{ErrorState, SettingsConfirmationState};
use crate::ui::state::typed::{TypedAppState, TypedStateChange};
use async_trait::async_trait;
use crossterm::event::{KeyCode, MouseEvent};
use ratatui::Frame;

/// All possible states for migration mode.
///
/// This enum provides type-safe state management for the migration workflow.
/// Each variant wraps a concrete state type, allowing direct dispatch
/// without virtual method calls.
///
/// # States
///
/// The migration workflow progresses through these states:
/// 1. `SettingsConfirmation` - Confirm settings before starting
/// 2. `DataLoading` - Fetch PRs, analyze commits, determine migration eligibility
/// 3. `Results` - Display categorized PRs (eligible, unsure, not merged)
/// 4. `VersionInput` - User enters version for tagging
/// 5. `Tagging` - Apply tags to eligible PRs
/// 6. `Error` - Display error messages
///
/// # Example
///
/// ```ignore
/// let mut state = MigrationModeState::DataLoading(
///     MigrationDataLoadingState::new(config)
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
pub enum MigrationModeState {
    /// Settings confirmation screen (boxed to reduce enum size).
    SettingsConfirmation(Box<SettingsConfirmationState>),
    /// Loading data and analyzing migration (boxed to reduce enum size).
    DataLoading(Box<MigrationDataLoadingState>),
    /// Migration results display.
    Results(MigrationResultsState),
    /// Version input for tagging.
    VersionInput(MigrationVersionInputState),
    /// Tagging eligible PRs.
    Tagging(MigrationTaggingState),
    /// Error display screen.
    Error(ErrorState),
}

impl std::fmt::Debug for MigrationModeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MigrationModeState::{}", self.name())
    }
}

impl MigrationModeState {
    /// Create the initial state for migration mode with data loading.
    ///
    /// # Arguments
    ///
    /// * `config` - The application configuration
    ///
    /// # Returns
    ///
    /// A `DataLoading` state to start the migration analysis.
    pub fn initial(config: crate::models::AppConfig) -> Self {
        MigrationModeState::DataLoading(Box::new(MigrationDataLoadingState::new(config)))
    }

    /// Create the initial state with settings confirmation.
    ///
    /// Returns `SettingsConfirmation` state if confirmation is required.
    pub fn initial_with_confirmation(config: crate::models::AppConfig) -> Self {
        MigrationModeState::SettingsConfirmation(Box::new(SettingsConfirmationState::new(config)))
    }

    /// Get the name of the current state for logging/debugging.
    pub fn name(&self) -> &'static str {
        match self {
            MigrationModeState::SettingsConfirmation(_) => "SettingsConfirmation",
            MigrationModeState::DataLoading(_) => "DataLoading",
            MigrationModeState::Results(_) => "Results",
            MigrationModeState::VersionInput(_) => "VersionInput",
            MigrationModeState::Tagging(_) => "Tagging",
            MigrationModeState::Error(_) => "Error",
        }
    }
}

// Note: The full TypedAppState implementation for MigrationModeState will be added
// in Phase 4 when individual states are migrated to use the typed interface.
// For now, we define the enum structure to establish the state machine.

/// Placeholder implementation for MigrationModeState.
///
/// This implementation will be completed in Phase 4 when states are migrated
/// to the typed interface. Currently, states still use the legacy AppState trait.
#[async_trait]
impl TypedAppState for MigrationModeState {
    type App = MigrationApp;
    type StateEnum = MigrationModeState;

    fn ui(&mut self, _f: &mut Frame, _app: &MigrationApp) {
        // Placeholder - will delegate to inner state in Phase 4
        // For now, states are rendered via the legacy AppState trait
        unimplemented!(
            "MigrationModeState::ui() - states are being migrated. Use legacy AppState for now."
        )
    }

    async fn process_key(
        &mut self,
        _code: KeyCode,
        _app: &mut MigrationApp,
    ) -> TypedStateChange<MigrationModeState> {
        // Placeholder - will delegate to inner state in Phase 4
        unimplemented!(
            "MigrationModeState::process_key() - states are being migrated. Use legacy AppState for now."
        )
    }

    async fn process_mouse(
        &mut self,
        _event: MouseEvent,
        _app: &mut MigrationApp,
    ) -> TypedStateChange<MigrationModeState> {
        // Placeholder - will delegate to inner state in Phase 4
        TypedStateChange::Keep
    }

    fn name(&self) -> &'static str {
        MigrationModeState::name(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppConfig, MigrationModeConfig, SharedConfig};
    use crate::parsed_property::ParsedProperty;

    fn create_test_migration_config() -> AppConfig {
        AppConfig::Migration {
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
            migration: MigrationModeConfig {
                terminal_states: ParsedProperty::Default(vec![
                    "Closed".to_string(),
                    "Resolved".to_string(),
                ]),
            },
        }
    }

    /// # MigrationModeState Initial State
    ///
    /// Tests that the initial state is DataLoading.
    ///
    /// ## Test Scenario
    /// - Calls MigrationModeState::initial()
    ///
    /// ## Expected Outcome
    /// - Should return DataLoading variant
    #[test]
    fn test_migration_mode_state_initial() {
        let config = create_test_migration_config();
        let state = MigrationModeState::initial(config);
        assert!(matches!(state, MigrationModeState::DataLoading(_)));
        assert_eq!(state.name(), "DataLoading");
    }

    /// # MigrationModeState Initial With Confirmation
    ///
    /// Tests that initial_with_confirmation returns SettingsConfirmation.
    ///
    /// ## Test Scenario
    /// - Creates a test config
    /// - Calls MigrationModeState::initial_with_confirmation()
    ///
    /// ## Expected Outcome
    /// - Should return SettingsConfirmation variant
    #[test]
    fn test_migration_mode_state_initial_with_confirmation() {
        let config = create_test_migration_config();
        let state = MigrationModeState::initial_with_confirmation(config);
        assert!(matches!(state, MigrationModeState::SettingsConfirmation(_)));
        assert_eq!(state.name(), "SettingsConfirmation");
    }

    /// # MigrationModeState Name For All Variants
    ///
    /// Tests that name() returns correct values for variants.
    ///
    /// ## Test Scenario
    /// - Creates various MigrationModeState variants
    /// - Checks the names
    ///
    /// ## Expected Outcome
    /// - Each variant should have a unique, correct name
    #[test]
    fn test_migration_mode_state_names() {
        let config = create_test_migration_config();

        let data_loading =
            MigrationModeState::DataLoading(Box::new(MigrationDataLoadingState::new(config)));
        assert_eq!(data_loading.name(), "DataLoading");

        let results = MigrationModeState::Results(MigrationResultsState::new());
        assert_eq!(results.name(), "Results");

        let error = MigrationModeState::Error(ErrorState::new());
        assert_eq!(error.name(), "Error");
    }

    /// # MigrationModeState Debug Implementation
    ///
    /// Tests that MigrationModeState implements Debug correctly.
    ///
    /// ## Test Scenario
    /// - Creates a MigrationModeState
    /// - Formats it using Debug
    ///
    /// ## Expected Outcome
    /// - Should produce readable debug output
    #[test]
    fn test_migration_mode_state_debug() {
        let config = create_test_migration_config();
        let state = MigrationModeState::initial(config);
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("DataLoading"));
    }
}
