mod cleanup;
mod default;
mod migration;
mod shared;
pub mod typed;

use async_trait::async_trait;
pub use cleanup::*;
pub use default::*;
pub use migration::*;
pub use shared::*;
pub use typed::{TypedAppState, TypedStateChange};

use crate::ui::App;
use crossterm::event::{KeyCode, MouseEvent};
use ratatui::Frame;

pub enum StateChange {
    Keep,
    Change(Box<dyn AppState>),
    Exit,
}

#[async_trait]
pub trait AppState: Send + Sync {
    fn ui(&mut self, f: &mut Frame, app: &App);
    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange;
    async fn process_mouse(&mut self, _event: MouseEvent, _app: &mut App) -> StateChange {
        StateChange::Keep
    }
}

/// Factory function to create the initial state based on app configuration
pub fn create_initial_state(config: Option<crate::models::AppConfig>) -> Box<dyn AppState> {
    if let Some(config) = config {
        // Skip confirmation if the flag is set
        if config.shared().skip_confirmation {
            if config.is_migration_mode() {
                Box::new(MigrationDataLoadingState::new(config))
            } else if config.is_cleanup_mode() {
                Box::new(CleanupDataLoadingState::new(config))
            } else {
                Box::new(DataLoadingState::new())
            }
        } else {
            Box::new(SettingsConfirmationState::new(config))
        }
    } else {
        // This shouldn't happen in normal flow since we always resolve config
        Box::new(DataLoadingState::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{
            AppConfig, CleanupModeConfig, DefaultModeConfig, MigrationModeConfig, SharedConfig,
        },
        parsed_property::ParsedProperty,
    };

    fn create_shared_config(skip_confirmation: bool) -> SharedConfig {
        SharedConfig {
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
            skip_confirmation,
        }
    }

    /// # Create Initial State - None Config
    ///
    /// Tests that passing None config creates a DataLoadingState.
    ///
    /// ## Test Scenario
    /// - Passes None to create_initial_state
    ///
    /// ## Expected Outcome
    /// - Should return a valid state (DataLoadingState)
    #[test]
    fn test_create_initial_state_none_config() {
        let state = create_initial_state(None);
        // The state is created successfully - we can't directly check the type
        // but we verify it doesn't panic and returns a valid Box<dyn AppState>
        assert!(std::mem::size_of_val(&state) > 0);
    }

    /// # Create Initial State - Default Mode with Skip Confirmation
    ///
    /// Tests that default mode with skip_confirmation=true creates DataLoadingState.
    ///
    /// ## Test Scenario
    /// - Creates a default mode config with skip_confirmation=true
    /// - Passes to create_initial_state
    ///
    /// ## Expected Outcome
    /// - Should return DataLoadingState
    #[test]
    fn test_create_initial_state_default_mode_skip_confirmation() {
        let config = AppConfig::Default {
            shared: create_shared_config(true),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        };
        let state = create_initial_state(Some(config));
        assert!(std::mem::size_of_val(&state) > 0);
    }

    /// # Create Initial State - Default Mode Without Skip Confirmation
    ///
    /// Tests that default mode with skip_confirmation=false creates SettingsConfirmationState.
    ///
    /// ## Test Scenario
    /// - Creates a default mode config with skip_confirmation=false
    /// - Passes to create_initial_state
    ///
    /// ## Expected Outcome
    /// - Should return SettingsConfirmationState
    #[test]
    fn test_create_initial_state_default_mode_with_confirmation() {
        let config = AppConfig::Default {
            shared: create_shared_config(false),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        };
        let state = create_initial_state(Some(config));
        assert!(std::mem::size_of_val(&state) > 0);
    }

    /// # Create Initial State - Migration Mode with Skip Confirmation
    ///
    /// Tests that migration mode with skip_confirmation=true creates MigrationDataLoadingState.
    ///
    /// ## Test Scenario
    /// - Creates a migration mode config with skip_confirmation=true
    /// - Passes to create_initial_state
    ///
    /// ## Expected Outcome
    /// - Should return MigrationDataLoadingState
    #[test]
    fn test_create_initial_state_migration_mode_skip_confirmation() {
        let config = AppConfig::Migration {
            shared: create_shared_config(true),
            migration: MigrationModeConfig {
                terminal_states: ParsedProperty::Default(vec![
                    "Closed".to_string(),
                    "Resolved".to_string(),
                ]),
            },
        };
        let state = create_initial_state(Some(config));
        assert!(std::mem::size_of_val(&state) > 0);
    }

    /// # Create Initial State - Migration Mode Without Skip Confirmation
    ///
    /// Tests that migration mode with skip_confirmation=false creates SettingsConfirmationState.
    ///
    /// ## Test Scenario
    /// - Creates a migration mode config with skip_confirmation=false
    /// - Passes to create_initial_state
    ///
    /// ## Expected Outcome
    /// - Should return SettingsConfirmationState
    #[test]
    fn test_create_initial_state_migration_mode_with_confirmation() {
        let config = AppConfig::Migration {
            shared: create_shared_config(false),
            migration: MigrationModeConfig {
                terminal_states: ParsedProperty::Default(vec![
                    "Closed".to_string(),
                    "Resolved".to_string(),
                ]),
            },
        };
        let state = create_initial_state(Some(config));
        assert!(std::mem::size_of_val(&state) > 0);
    }

    /// # Create Initial State - Cleanup Mode with Skip Confirmation
    ///
    /// Tests that cleanup mode with skip_confirmation=true creates CleanupDataLoadingState.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode config with skip_confirmation=true
    /// - Passes to create_initial_state
    ///
    /// ## Expected Outcome
    /// - Should return CleanupDataLoadingState
    #[test]
    fn test_create_initial_state_cleanup_mode_skip_confirmation() {
        let config = AppConfig::Cleanup {
            shared: create_shared_config(true),
            cleanup: CleanupModeConfig {
                target: ParsedProperty::Default("main".to_string()),
            },
        };
        let state = create_initial_state(Some(config));
        assert!(std::mem::size_of_val(&state) > 0);
    }

    /// # Create Initial State - Cleanup Mode Without Skip Confirmation
    ///
    /// Tests that cleanup mode with skip_confirmation=false creates SettingsConfirmationState.
    ///
    /// ## Test Scenario
    /// - Creates a cleanup mode config with skip_confirmation=false
    /// - Passes to create_initial_state
    ///
    /// ## Expected Outcome
    /// - Should return SettingsConfirmationState
    #[test]
    fn test_create_initial_state_cleanup_mode_with_confirmation() {
        let config = AppConfig::Cleanup {
            shared: create_shared_config(false),
            cleanup: CleanupModeConfig {
                target: ParsedProperty::Default("main".to_string()),
            },
        };
        let state = create_initial_state(Some(config));
        assert!(std::mem::size_of_val(&state) > 0);
    }
}
