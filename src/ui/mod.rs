use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use state::{AppState, DataLoadingState, StateChange};

/// Macro to process state changes and handle Keep/Change/Exit
macro_rules! handle_state_change {
    ($result:expr, $current_state:expr) => {
        match $result {
            StateChange::Keep => {}
            StateChange::Change(new_state) => {
                $current_state = new_state;
            }
            StateChange::Exit => break,
        }
    };
}

mod app;
mod app_base;
mod app_mode;
pub mod apps;
mod events;
#[cfg(test)]
pub mod snapshot_testing;
pub mod state;
#[cfg(test)]
pub mod testing;
mod worktree_context;

pub use app::App;
pub use app_base::AppBase;
pub use app_mode::AppMode;
pub use apps::{CleanupApp, MergeApp, MigrationApp};
#[cfg(test)]
pub use events::testing::MockEventSource;
pub use events::{CrosstermEventSource, EventSource};
pub use worktree_context::WorktreeContext;

/// Run the application loop with an injectable event source.
///
/// This is the core application loop that:
/// 1. Draws the current state to the terminal
/// 2. Polls for events using the provided event source
/// 3. Processes events through the state machine
/// 4. Handles state transitions until `StateChange::Exit`
///
/// # Arguments
///
/// * `terminal` - The terminal to draw to
/// * `app` - The application state
/// * `event_source` - The source of terminal events (can be mocked for testing)
///
/// # Example
///
/// ```ignore
/// // Production usage
/// run_app_with_events(&mut terminal, &mut app, &CrosstermEventSource::new()).await?;
///
/// // Test usage
/// let events = MockEventSource::new().with_key(KeyCode::Char('q'));
/// run_app_with_events(&mut terminal, &mut app, &events).await?;
/// ```
pub async fn run_app_with_events<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    event_source: &dyn EventSource,
) -> anyhow::Result<()> {
    let mut current_state: Box<dyn AppState> = app
        .initial_state
        .take()
        .unwrap_or_else(|| Box::new(DataLoadingState::new()));

    loop {
        terminal.draw(|f| current_state.ui(f, app))?;

        // Use poll with timeout to allow states to execute immediately
        if event_source.poll(std::time::Duration::from_millis(50))? {
            match event_source.read()? {
                Event::Key(key) => {
                    handle_state_change!(
                        current_state.process_key(key.code, app).await,
                        current_state
                    );
                }
                Event::Mouse(mouse) => {
                    handle_state_change!(
                        current_state.process_mouse(mouse, app).await,
                        current_state
                    );
                }
                _ => {}
            }
        } else {
            // No event, but still allow state to process (for immediate execution)
            handle_state_change!(
                current_state.process_key(KeyCode::Null, app).await,
                current_state
            );
        }
    }

    Ok(())
}

/// Run the application loop using the default crossterm event source.
///
/// This is a convenience wrapper around [`run_app_with_events`] for production use.
/// It uses [`CrosstermEventSource`] to read actual terminal events.
///
/// # Arguments
///
/// * `terminal` - The terminal to draw to
/// * `app` - The application state
pub async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> anyhow::Result<()> {
    run_app_with_events(terminal, app, &CrosstermEventSource::new()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::ErrorState;
    use crate::ui::testing::{TuiTestHarness, create_test_config_default};
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    /// # Run App Exit with Q Key
    ///
    /// Tests that the app loop exits cleanly when 'q' is pressed.
    ///
    /// ## Test Scenario
    /// - Start with ErrorState (simple state that exits on 'q')
    /// - Send 'q' key event
    ///
    /// ## Expected Outcome
    /// - App loop should exit with Ok(())
    #[tokio::test]
    async fn test_run_app_exit_with_q_key() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
        assert!(events.is_empty(), "All events should be consumed");
    }

    /// # Run App Exit with Escape Key
    ///
    /// Tests that the app handles Escape key (depends on state).
    ///
    /// ## Test Scenario
    /// - Start with ErrorState
    /// - Send Escape key, then 'q' key
    ///
    /// ## Expected Outcome
    /// - ErrorState ignores Escape, then exits on 'q'
    #[tokio::test]
    async fn test_run_app_escape_then_quit() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::Esc)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Navigation Before Exit
    ///
    /// Tests that navigation keys are processed before exit.
    ///
    /// ## Test Scenario
    /// - Start with ErrorState
    /// - Send multiple navigation keys, then 'q'
    ///
    /// ## Expected Outcome
    /// - All keys should be processed, then exit on 'q'
    #[tokio::test]
    async fn test_run_app_navigation_before_exit() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::Down)
            .with_key(KeyCode::Up)
            .with_key(KeyCode::Left)
            .with_key(KeyCode::Right)
            .with_key(KeyCode::Home)
            .with_key(KeyCode::End)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
        assert!(events.is_empty());
    }

    /// # Run App Timeout Processing
    ///
    /// Tests that poll timeouts trigger KeyCode::Null processing.
    ///
    /// ## Test Scenario
    /// - Start with ErrorState
    /// - Send timeouts (poll returns false), then 'q' key
    ///
    /// ## Expected Outcome
    /// - Timeouts should be processed as KeyCode::Null
    /// - App should exit cleanly on 'q'
    #[tokio::test]
    async fn test_run_app_timeout_processing() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_timeout()
            .with_timeout()
            .with_timeout()
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Mouse Event Processing
    ///
    /// Tests that mouse events are passed to the state.
    ///
    /// ## Test Scenario
    /// - Start with ErrorState
    /// - Send mouse click event, then 'q' key
    ///
    /// ## Expected Outcome
    /// - Mouse event should be processed (ErrorState ignores it)
    /// - App should exit cleanly on 'q'
    #[tokio::test]
    async fn test_run_app_mouse_event() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let mouse_event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };

        let events = MockEventSource::new()
            .with_mouse(mouse_event)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Multiple Mouse Events
    ///
    /// Tests multiple mouse events in sequence.
    ///
    /// ## Test Scenario
    /// - Start with ErrorState
    /// - Send multiple mouse events (click, drag, scroll)
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - All mouse events should be processed
    #[tokio::test]
    async fn test_run_app_multiple_mouse_events() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new();

        // Click
        events.push_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });

        // Release
        events.push_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });

        // Scroll
        events.push_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });

        events.push_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Mixed Events
    ///
    /// Tests a mix of key, mouse, and timeout events.
    ///
    /// ## Test Scenario
    /// - Start with ErrorState
    /// - Send keys, timeouts, and mouse events interleaved
    ///
    /// ## Expected Outcome
    /// - All events should be processed in order
    #[tokio::test]
    async fn test_run_app_mixed_events() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::Down)
            .with_timeout()
            .with_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 3,
                modifiers: KeyModifiers::NONE,
            })
            .with_key(KeyCode::Up)
            .with_timeout()
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Default Initial State
    ///
    /// Tests that default DataLoadingState is used when no initial state is set.
    ///
    /// ## Test Scenario
    /// - Create app without setting initial_state
    /// - Send 'q' key to exit
    ///
    /// ## Expected Outcome
    /// - DataLoadingState should be used as default
    /// - App should exit cleanly
    #[tokio::test]
    async fn test_run_app_default_initial_state() {
        let mut harness = TuiTestHarness::new();
        // Don't set initial_state, let it use default DataLoadingState

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App With Error Message Display
    ///
    /// Tests that error messages are displayed when set on app.
    ///
    /// ## Test Scenario
    /// - Set error message on app
    /// - Start with ErrorState
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - Error should be rendered (terminal should have content)
    /// - App should exit cleanly
    #[tokio::test]
    async fn test_run_app_with_error_message() {
        let mut harness = TuiTestHarness::new()
            .with_error_message("Test error message for display")
            .with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());

        // Verify something was drawn to the terminal
        let buffer = harness.terminal.backend().buffer();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("Error") || content.contains("error"));
    }

    /// # Run App Key Modifiers
    ///
    /// Tests that key events with modifiers are handled.
    ///
    /// ## Test Scenario
    /// - Send key with Control modifier
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - Modified key should be processed
    #[tokio::test]
    async fn test_run_app_key_with_modifiers() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key_modified(KeyCode::Char('c'), KeyModifiers::CONTROL)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Function Keys
    ///
    /// Tests that function keys are handled.
    ///
    /// ## Test Scenario
    /// - Send various function keys
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - Function keys should be processed
    #[tokio::test]
    async fn test_run_app_function_keys() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::F(1))
            .with_key(KeyCode::F(5))
            .with_key(KeyCode::F(12))
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Tab and Backtab
    ///
    /// Tests Tab and BackTab key handling.
    ///
    /// ## Test Scenario
    /// - Send Tab and BackTab keys
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - Tab keys should be processed
    #[tokio::test]
    async fn test_run_app_tab_keys() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::Tab)
            .with_key(KeyCode::BackTab)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Enter Key
    ///
    /// Tests Enter key handling.
    ///
    /// ## Test Scenario
    /// - Send Enter key
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - Enter key should be processed
    #[tokio::test]
    async fn test_run_app_enter_key() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::Enter)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Page Navigation
    ///
    /// Tests PageUp and PageDown key handling.
    ///
    /// ## Test Scenario
    /// - Send page navigation keys
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - Page keys should be processed
    #[tokio::test]
    async fn test_run_app_page_keys() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::PageUp)
            .with_key(KeyCode::PageDown)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Backspace and Delete
    ///
    /// Tests Backspace and Delete key handling.
    ///
    /// ## Test Scenario
    /// - Send Backspace and Delete keys
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - Keys should be processed
    #[tokio::test]
    async fn test_run_app_backspace_delete() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::Backspace)
            .with_key(KeyCode::Delete)
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App Empty Events Auto Exit
    ///
    /// Tests that when events are exhausted, poll returns false.
    ///
    /// ## Test Scenario
    /// - Add only a quit event
    /// - Verify loop exits
    ///
    /// ## Expected Outcome
    /// - App should exit after processing all events
    #[tokio::test]
    async fn test_run_app_events_exhausted() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
        assert!(events.is_empty());
    }

    /// # Run App Using Harness Helper
    ///
    /// Tests the TuiTestHarness run_with_events helper method.
    ///
    /// ## Test Scenario
    /// - Use harness helper instead of direct function call
    ///
    /// ## Expected Outcome
    /// - Helper should work the same as direct call
    #[tokio::test]
    async fn test_run_app_using_harness_helper() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let result = harness.run_with_events(&events).await;

        assert!(result.is_ok());
    }

    /// # Run App Using Keys Helper
    ///
    /// Tests the TuiTestHarness run_with_keys helper method.
    ///
    /// ## Test Scenario
    /// - Use keys helper with a vector of key codes
    ///
    /// ## Expected Outcome
    /// - Keys helper should work correctly
    #[tokio::test]
    async fn test_run_app_using_keys_helper() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let result = harness
            .run_with_keys(vec![KeyCode::Down, KeyCode::Up, KeyCode::Char('q')])
            .await;

        assert!(result.is_ok());
    }

    /// # Run App Rapid Key Presses
    ///
    /// Tests rapid sequential key presses.
    ///
    /// ## Test Scenario
    /// - Send many keys in rapid succession
    ///
    /// ## Expected Outcome
    /// - All keys should be processed correctly
    #[tokio::test]
    async fn test_run_app_rapid_keys() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new();
        for _ in 0..50 {
            events.push_key(KeyCode::Down);
        }
        events.push_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
        assert!(events.is_empty());
    }

    /// # Run App Character Keys
    ///
    /// Tests various character key inputs.
    ///
    /// ## Test Scenario
    /// - Send various character keys (letters, numbers, symbols)
    /// - Exit with 'q'
    ///
    /// ## Expected Outcome
    /// - All character keys should be processed
    #[tokio::test]
    async fn test_run_app_character_keys() {
        let mut harness = TuiTestHarness::new().with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new()
            .with_key(KeyCode::Char('a'))
            .with_key(KeyCode::Char('z'))
            .with_key(KeyCode::Char('0'))
            .with_key(KeyCode::Char('9'))
            .with_key(KeyCode::Char('!'))
            .with_key(KeyCode::Char('@'))
            .with_key(KeyCode::Char(' '))
            .with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }

    /// # Run App With Custom Config
    ///
    /// Tests running with a custom configuration.
    ///
    /// ## Test Scenario
    /// - Create harness with custom config
    /// - Run with events
    ///
    /// ## Expected Outcome
    /// - App should use the custom config
    #[tokio::test]
    async fn test_run_app_with_custom_config() {
        let config = create_test_config_default();
        let mut harness =
            TuiTestHarness::with_config(config).with_initial_state(Box::new(ErrorState::new()));

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let result = run_app_with_events(&mut harness.terminal, &mut harness.app, &events).await;

        assert!(result.is_ok());
    }
}
