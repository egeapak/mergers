mod cherry_pick;
mod completion;
mod conflict_resolution;
mod data_loading;
mod error;
mod migration;
mod migration_loading;
mod post_completion;
mod pr_selection;
mod setup_repo;
mod version_input;

use async_trait::async_trait;
pub use cherry_pick::CherryPickState;
pub use completion::CompletionState;
pub use conflict_resolution::ConflictResolutionState;
pub use data_loading::DataLoadingState;
pub use error::ErrorState;
pub use migration::MigrationState;
pub use migration_loading::MigrationLoadingState;
pub use post_completion::PostCompletionState;
pub use pr_selection::PullRequestSelectionState;
pub use setup_repo::SetupRepoState;
pub use version_input::VersionInputState;

use crate::ui::App;
use crossterm::event::KeyCode;
use ratatui::Frame;

pub enum StateChange {
    Keep,
    Change(Box<dyn AppState>),
    Exit,
}

#[async_trait]
pub trait AppState {
    fn ui(&mut self, f: &mut Frame, app: &App);
    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange;
}
