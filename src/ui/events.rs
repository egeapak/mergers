//! Event source abstraction for terminal events
//!
//! This module provides a trait-based abstraction over crossterm's event system,
//! enabling dependency injection for testing the main application loop.

use crossterm::event::Event;
use std::io;
use std::time::Duration;

/// Trait for abstracting terminal event sources.
///
/// This enables mocking of crossterm events for testing without requiring
/// actual terminal input. The trait mirrors crossterm's `event::poll` and
/// `event::read` functions.
pub trait EventSource: Send + Sync {
    /// Check if an event is available within the timeout.
    ///
    /// Returns `true` if an event is ready to be read, `false` if the timeout
    /// elapsed without an event becoming available.
    fn poll(&self, timeout: Duration) -> io::Result<bool>;

    /// Read the next event.
    ///
    /// This may block if no event is immediately available (depending on
    /// implementation). For the mock implementation, returns an error if
    /// the event queue is empty.
    fn read(&self) -> io::Result<Event>;
}

/// Production event source using crossterm.
///
/// This is a thin wrapper around crossterm's global event functions,
/// used in production to read actual terminal events.
#[derive(Debug, Clone, Copy, Default)]
pub struct CrosstermEventSource;

impl CrosstermEventSource {
    /// Create a new CrosstermEventSource.
    pub fn new() -> Self {
        Self
    }
}

impl EventSource for CrosstermEventSource {
    fn poll(&self, timeout: Duration) -> io::Result<bool> {
        crossterm::event::poll(timeout)
    }

    fn read(&self) -> io::Result<Event> {
        crossterm::event::read()
    }
}

#[cfg(test)]
pub mod testing {
    //! Test utilities for mocking terminal events.

    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseEvent,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// Mock event source for testing.
    ///
    /// Allows injecting a sequence of events that will be returned by
    /// `poll` and `read` calls. Events are consumed in FIFO order.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let events = MockEventSource::new()
    ///     .with_key(KeyCode::Down)
    ///     .with_key(KeyCode::Enter)
    ///     .with_key(KeyCode::Char('q'));
    ///
    /// // Use with run_app_with_events for testing
    /// ```
    pub struct MockEventSource {
        events: Mutex<VecDeque<Event>>,
        poll_returns: Mutex<VecDeque<bool>>,
    }

    impl Default for MockEventSource {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockEventSource {
        /// Create a new empty MockEventSource.
        pub fn new() -> Self {
            Self {
                events: Mutex::new(VecDeque::new()),
                poll_returns: Mutex::new(VecDeque::new()),
            }
        }

        /// Add a key event to the queue.
        ///
        /// This also adds a `true` to the poll queue so that the event
        /// will be read on the next poll/read cycle.
        pub fn push_key(&self, code: KeyCode) {
            let event = Event::Key(KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            });
            self.events.lock().unwrap().push_back(event);
            self.poll_returns.lock().unwrap().push_back(true);
        }

        /// Add a key event with modifiers to the queue.
        pub fn push_key_with_modifiers(&self, code: KeyCode, modifiers: KeyModifiers) {
            let event = Event::Key(KeyEvent {
                code,
                modifiers,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            });
            self.events.lock().unwrap().push_back(event);
            self.poll_returns.lock().unwrap().push_back(true);
        }

        /// Add a mouse event to the queue.
        pub fn push_mouse(&self, event: MouseEvent) {
            self.events.lock().unwrap().push_back(Event::Mouse(event));
            self.poll_returns.lock().unwrap().push_back(true);
        }

        /// Add a poll timeout (no event available).
        ///
        /// This causes `poll` to return `false`, which triggers the
        /// `KeyCode::Null` processing path in the app loop.
        pub fn push_timeout(&self) {
            self.poll_returns.lock().unwrap().push_back(false);
        }

        /// Builder pattern: add a key event and return self.
        pub fn with_key(self, code: KeyCode) -> Self {
            self.push_key(code);
            self
        }

        /// Builder pattern: add a key event with modifiers and return self.
        pub fn with_key_modified(self, code: KeyCode, modifiers: KeyModifiers) -> Self {
            self.push_key_with_modifiers(code, modifiers);
            self
        }

        /// Builder pattern: add a timeout and return self.
        pub fn with_timeout(self) -> Self {
            self.push_timeout();
            self
        }

        /// Builder pattern: add a mouse event and return self.
        pub fn with_mouse(self, event: MouseEvent) -> Self {
            self.push_mouse(event);
            self
        }

        /// Check if all events have been consumed.
        pub fn is_empty(&self) -> bool {
            self.events.lock().unwrap().is_empty() && self.poll_returns.lock().unwrap().is_empty()
        }

        /// Get the number of remaining events.
        pub fn remaining_events(&self) -> usize {
            self.events.lock().unwrap().len()
        }

        /// Get the number of remaining poll returns.
        pub fn remaining_polls(&self) -> usize {
            self.poll_returns.lock().unwrap().len()
        }
    }

    impl EventSource for MockEventSource {
        fn poll(&self, _timeout: Duration) -> io::Result<bool> {
            // Return the next poll result, defaulting to false (no event)
            Ok(self
                .poll_returns
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(false))
        }

        fn read(&self) -> io::Result<Event> {
            self.events
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "No events in queue"))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// # MockEventSource Basic Operations
        ///
        /// Tests basic push and consume operations.
        ///
        /// ## Test Scenario
        /// - Push keys and timeouts
        /// - Verify poll and read return expected values
        ///
        /// ## Expected Outcome
        /// - Events are returned in FIFO order
        #[test]
        fn test_mock_event_source_basic() {
            let source = MockEventSource::new()
                .with_key(KeyCode::Down)
                .with_timeout()
                .with_key(KeyCode::Enter);

            // First: key event
            assert!(source.poll(Duration::from_millis(0)).unwrap());
            let event = source.read().unwrap();
            assert!(matches!(event, Event::Key(k) if k.code == KeyCode::Down));

            // Second: timeout
            assert!(!source.poll(Duration::from_millis(0)).unwrap());

            // Third: key event
            assert!(source.poll(Duration::from_millis(0)).unwrap());
            let event = source.read().unwrap();
            assert!(matches!(event, Event::Key(k) if k.code == KeyCode::Enter));

            // Queue should be empty
            assert!(source.is_empty());
        }

        /// # MockEventSource Empty Queue
        ///
        /// Tests behavior when reading from an empty queue.
        ///
        /// ## Test Scenario
        /// - Create empty source
        /// - Attempt to read
        ///
        /// ## Expected Outcome
        /// - poll returns false
        /// - read returns WouldBlock error
        #[test]
        fn test_mock_event_source_empty() {
            let source = MockEventSource::new();

            assert!(!source.poll(Duration::from_millis(0)).unwrap());
            assert!(source.read().is_err());
            assert!(source.is_empty());
        }

        /// # MockEventSource Mouse Events
        ///
        /// Tests mouse event handling.
        ///
        /// ## Test Scenario
        /// - Push a mouse event
        /// - Verify it's returned correctly
        ///
        /// ## Expected Outcome
        /// - Mouse event is returned with correct properties
        #[test]
        fn test_mock_event_source_mouse() {
            use crossterm::event::{MouseButton, MouseEventKind};

            let mouse_event = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: KeyModifiers::NONE,
            };

            let source = MockEventSource::new().with_mouse(mouse_event);

            assert!(source.poll(Duration::from_millis(0)).unwrap());
            let event = source.read().unwrap();
            assert!(matches!(event, Event::Mouse(m) if m.column == 10 && m.row == 5));
        }

        /// # MockEventSource Key with Modifiers
        ///
        /// Tests key events with modifiers.
        ///
        /// ## Test Scenario
        /// - Push Ctrl+C key event
        /// - Verify modifiers are preserved
        ///
        /// ## Expected Outcome
        /// - Key event has correct modifiers
        #[test]
        fn test_mock_event_source_key_with_modifiers() {
            let source =
                MockEventSource::new().with_key_modified(KeyCode::Char('c'), KeyModifiers::CONTROL);

            assert!(source.poll(Duration::from_millis(0)).unwrap());
            let event = source.read().unwrap();

            if let Event::Key(key) = event {
                assert_eq!(key.code, KeyCode::Char('c'));
                assert!(key.modifiers.contains(KeyModifiers::CONTROL));
            } else {
                panic!("Expected key event");
            }
        }

        /// # MockEventSource Remaining Counts
        ///
        /// Tests the remaining event/poll count methods.
        ///
        /// ## Test Scenario
        /// - Add events and check counts
        /// - Consume events and verify counts decrease
        ///
        /// ## Expected Outcome
        /// - Counts accurately reflect queue state
        #[test]
        fn test_mock_event_source_counts() {
            let source = MockEventSource::new()
                .with_key(KeyCode::Down)
                .with_key(KeyCode::Up)
                .with_timeout();

            assert_eq!(source.remaining_events(), 2);
            assert_eq!(source.remaining_polls(), 3);

            source.poll(Duration::from_millis(0)).unwrap();
            source.read().unwrap();

            assert_eq!(source.remaining_events(), 1);
            assert_eq!(source.remaining_polls(), 2);
        }
    }
}
