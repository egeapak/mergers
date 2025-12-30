//! Typed error state that works with any app mode.
//!
//! This module provides [`TypedErrorState`], a generic error display state
//! that can work with any mode-specific app type (MergeApp, MigrationApp, CleanupApp).

use crate::ui::AppMode;
use crate::ui::state::typed::StateChange;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::marker::PhantomData;

/// Typed error state - works with any app mode.
///
/// This state displays error messages from the app's shared state.
/// It is generic over the app type, allowing it to be used with
/// any mode (Merge, Migration, Cleanup).
///
/// # Type Parameters
///
/// * `A` - The app mode type (must implement [`AppMode`])
/// * `S` - The state enum type for this mode
///
/// # Example
///
/// ```ignore
/// use crate::ui::apps::MergeApp;
/// use crate::ui::state::default::MergeState;
///
/// // Create typed error state for merge mode
/// let error_state: TypedErrorState<MergeApp, MergeState> = TypedErrorState::new();
///
/// // It will access app.error_message via AppBase through Deref
/// ```
pub struct TypedErrorState<A, S> {
    _app: PhantomData<A>,
    _state: PhantomData<S>,
}

impl<A, S> std::fmt::Debug for TypedErrorState<A, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedErrorState").finish()
    }
}

impl<A, S> Default for TypedErrorState<A, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A, S> TypedErrorState<A, S> {
    /// Create a new typed error state.
    pub fn new() -> Self {
        Self {
            _app: PhantomData,
            _state: PhantomData,
        }
    }
}

impl<A, S> TypedErrorState<A, S>
where
    A: AppMode + Send + Sync,
    S: Send + Sync + 'static,
{
    /// Render the error UI.
    pub fn render(&self, f: &mut Frame, app: &A) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area());

        let title = Paragraph::new("Error Occurred")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Access error_message from AppBase via AppMode trait
        let error_msg = app
            .base()
            .error_message
            .as_deref()
            .unwrap_or("Unknown error");
        let error = Paragraph::new(error_msg)
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(error, chunks[1]);

        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_text = vec![Line::from(vec![
            Span::raw("Press "),
            Span::styled("q", key_style),
            Span::raw(" to exit"),
        ])];
        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(help, chunks[2]);
    }

    /// Handle key input.
    pub fn handle_key<R>(&self, code: KeyCode) -> StateChange<R> {
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }

    /// Get this state's name for logging/debugging.
    pub fn name(&self) -> &'static str {
        "Error"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # TypedErrorState Default Implementation
    ///
    /// Tests that TypedErrorState implements Default correctly.
    ///
    /// ## Test Scenario
    /// - Creates a TypedErrorState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should create successfully
    #[test]
    fn test_typed_error_state_default() {
        let state: TypedErrorState<
            crate::ui::apps::MergeApp,
            crate::ui::state::default::MergeState,
        > = TypedErrorState::default();
        assert_eq!(state.name(), "Error");
    }

    /// # TypedErrorState New Constructor
    ///
    /// Tests that TypedErrorState::new() works correctly.
    ///
    /// ## Test Scenario
    /// - Creates a TypedErrorState using new()
    ///
    /// ## Expected Outcome
    /// - Should create successfully and have correct name
    #[test]
    fn test_typed_error_state_new() {
        let state: TypedErrorState<
            crate::ui::apps::MergeApp,
            crate::ui::state::default::MergeState,
        > = TypedErrorState::new();
        assert_eq!(state.name(), "Error");
    }

    /// # TypedErrorState Debug Implementation
    ///
    /// Tests that TypedErrorState implements Debug correctly.
    ///
    /// ## Test Scenario
    /// - Creates a TypedErrorState
    /// - Formats it using Debug
    ///
    /// ## Expected Outcome
    /// - Should produce readable debug output
    #[test]
    fn test_typed_error_state_debug() {
        let state: TypedErrorState<
            crate::ui::apps::MergeApp,
            crate::ui::state::default::MergeState,
        > = TypedErrorState::new();
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("TypedErrorState"));
    }
}
