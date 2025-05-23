use crossterm::event::KeyCode;
use ratatui::Frame;

use super::app::App;

pub mod choose;

pub trait InitialState: AppState {}

pub enum StateChange {
    Keep,
    Change(Box<dyn AppState>),
    Exit,
}

pub trait AppState {
    fn ui(&mut self, f: &mut Frame, app: &App);
    fn process_key(&mut self, code: KeyCode, app: &App) -> StateChange;
}
