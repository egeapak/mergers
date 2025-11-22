mod cleanup;
mod default;
mod migration;
mod shared;

use async_trait::async_trait;
pub use cleanup::*;
pub use default::*;
pub use migration::*;
pub use shared::*;

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
