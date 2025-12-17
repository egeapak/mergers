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
