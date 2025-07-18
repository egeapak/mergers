mod default;
mod migration;
mod shared;

use async_trait::async_trait;
pub use default::*;
pub use migration::*;
pub use shared::*;

use crate::ui::App;
use crossterm::event::KeyCode;
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
}

/// Factory function to create the initial state based on app configuration
pub fn create_initial_state(config: Option<crate::models::AppConfig>) -> Box<dyn AppState> {
    if let Some(config) = config {
        if config.is_migration_mode() {
            Box::new(MigrationDataLoadingState::new(config))
        } else {
            Box::new(DataLoadingState::new())
        }
    } else {
        Box::new(DataLoadingState::new())
    }
}
