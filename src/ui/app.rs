use chrono::DateTime;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::{io, process::Command};

use crate::models::{PullRequestWithWorkItems, WorkItem};

use super::state::{AppState, choose::ChooseState};

pub struct App {
    pub state: Option<Box<dyn AppState>>,
    pub organization: String,
    pub project: String,
    pub repository: String,
}

impl App {
    pub fn new(
        pull_requests: Vec<PullRequestWithWorkItems>,
        organization: String,
        project: String,
        repository: String,
    ) -> Self {
        let state = Box::new(ChooseState::new(pull_requests));
        Self {
            state: Some(state),
            organization,
            project,
            repository,
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let mut state = self.state.take().unwrap();
        state.ui(f, &self);
        self.state = Some(state);
    }

    pub fn process_key(&mut self, key: KeyEvent) -> Option<Vec<usize>> {
        let mut state = self.state.take().unwrap();
        match state.process_key(key.code, self) {
            super::state::StateChange::Keep => self.state = Some(state),
            super::state::StateChange::Change(app_state) => self.state = Some(app_state),
            super::state::StateChange::Exit => return Some(vec![]),
        };
        None
    }
}
