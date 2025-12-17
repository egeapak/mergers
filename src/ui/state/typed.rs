//! Typed app state infrastructure with associated types.
//!
//! This module provides the new type-safe state infrastructure that uses
//! associated types to ensure states receive correctly-typed app instances.
//! This exists alongside the legacy [`AppState`] trait during migration.
//!
//! # Architecture
//!
//! The key components are:
//! - [`TypedAppState`]: Trait with associated `App` type for compile-time safety
//! - [`TypedStateChange`]: Generic state change enum for type-safe transitions
//!
//! # Migration Path
//!
//! States are being migrated from the legacy `AppState` trait (which uses `&App`)
//! to `TypedAppState` (which uses `&Self::App`). During this transition:
//! - Legacy states continue using `AppState` with the old `App` struct
//! - New/migrated states implement `TypedAppState` with mode-specific apps
//!
//! [`AppState`]: super::AppState

use crate::ui::AppMode;
use async_trait::async_trait;
use crossterm::event::{KeyCode, MouseEvent};
use ratatui::Frame;

/// State change result from typed state operations.
///
/// Generic over the state type `S` to provide type-safe transitions
/// within a single mode. For example, `TypedStateChange<MergeState>`
/// ensures all transitions stay within the merge mode's state machine.
///
/// # Type Parameter
///
/// * `S` - The state enum type (e.g., `MergeState`, `MigrationState`)
#[derive(Debug)]
pub enum TypedStateChange<S> {
    /// Keep the current state unchanged.
    Keep,
    /// Change to a new state.
    Change(S),
    /// Exit the application.
    Exit,
}

impl<S> TypedStateChange<S> {
    /// Returns `true` if this is a `Keep` variant.
    pub fn is_keep(&self) -> bool {
        matches!(self, TypedStateChange::Keep)
    }

    /// Returns `true` if this is a `Change` variant.
    pub fn is_change(&self) -> bool {
        matches!(self, TypedStateChange::Change(_))
    }

    /// Returns `true` if this is an `Exit` variant.
    pub fn is_exit(&self) -> bool {
        matches!(self, TypedStateChange::Exit)
    }

    /// Maps the state type using a conversion function.
    ///
    /// Useful for converting between state representations.
    pub fn map<T, F>(self, f: F) -> TypedStateChange<T>
    where
        F: FnOnce(S) -> T,
    {
        match self {
            TypedStateChange::Keep => TypedStateChange::Keep,
            TypedStateChange::Change(s) => TypedStateChange::Change(f(s)),
            TypedStateChange::Exit => TypedStateChange::Exit,
        }
    }
}

/// Trait for typed UI states with compile-time app type safety.
///
/// This trait uses an associated type to specify which app mode a state
/// works with. This provides compile-time type checking - a state that
/// declares `type App = MergeApp` will only receive `MergeApp` instances,
/// preventing accidental access to wrong mode's fields.
///
/// # Associated Type
///
/// The `App` associated type must implement [`AppMode`], which guarantees
/// access to shared state through [`AppBase`]. Mode-specific apps also
/// implement `Deref<Target = AppBase>` for ergonomic access.
///
/// # Example
///
/// ```ignore
/// use crate::ui::apps::MergeApp;
/// use crate::ui::state::{TypedAppState, TypedStateChange};
///
/// struct CherryPickState {
///     current_index: usize,
/// }
///
/// #[async_trait]
/// impl TypedAppState for CherryPickState {
///     type App = MergeApp;
///     type StateEnum = MergeState;
///
///     fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
///         // Direct access to MergeApp fields
///         let items = &app.cherry_pick_items;
///         // Shared fields via Deref
///         let org = app.organization();
///     }
///
///     async fn process_key(
///         &mut self,
///         code: KeyCode,
///         app: &mut MergeApp
///     ) -> TypedStateChange<MergeState> {
///         // Type-safe state transitions
///         TypedStateChange::Keep
///     }
///
///     fn name(&self) -> &'static str { "CherryPick" }
/// }
/// ```
///
/// [`AppBase`]: crate::ui::AppBase
#[async_trait]
pub trait TypedAppState: Send + Sync {
    /// The app type this state works with.
    ///
    /// Must implement [`AppMode`] to ensure access to shared state.
    type App: AppMode + Send + Sync;

    /// The state enum type for this mode.
    ///
    /// Used for type-safe state transitions within a mode.
    type StateEnum: Send;

    /// Render the state's UI.
    ///
    /// # Arguments
    ///
    /// * `f` - The frame to render into
    /// * `app` - Reference to the mode-specific app
    fn ui(&mut self, f: &mut Frame, app: &Self::App);

    /// Process keyboard input.
    ///
    /// # Arguments
    ///
    /// * `code` - The key code that was pressed
    /// * `app` - Mutable reference to the mode-specific app
    ///
    /// # Returns
    ///
    /// A state change indicating whether to keep, change, or exit
    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut Self::App,
    ) -> TypedStateChange<Self::StateEnum>;

    /// Process mouse input.
    ///
    /// Default implementation returns `Keep` (no-op).
    ///
    /// # Arguments
    ///
    /// * `event` - The mouse event
    /// * `app` - Mutable reference to the mode-specific app
    ///
    /// # Returns
    ///
    /// A state change indicating whether to keep, change, or exit
    async fn process_mouse(
        &mut self,
        _event: MouseEvent,
        _app: &mut Self::App,
    ) -> TypedStateChange<Self::StateEnum> {
        TypedStateChange::Keep
    }

    /// Get this state's name for logging/debugging.
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # TypedStateChange Keep Variant
    ///
    /// Tests the Keep variant of TypedStateChange.
    ///
    /// ## Test Scenario
    /// - Creates a TypedStateChange::Keep
    /// - Checks the helper methods
    ///
    /// ## Expected Outcome
    /// - is_keep() returns true
    /// - is_change() returns false
    /// - is_exit() returns false
    #[test]
    fn test_typed_state_change_keep() {
        let change: TypedStateChange<String> = TypedStateChange::Keep;
        assert!(change.is_keep());
        assert!(!change.is_change());
        assert!(!change.is_exit());
    }

    /// # TypedStateChange Change Variant
    ///
    /// Tests the Change variant of TypedStateChange.
    ///
    /// ## Test Scenario
    /// - Creates a TypedStateChange::Change with a value
    /// - Checks the helper methods
    ///
    /// ## Expected Outcome
    /// - is_keep() returns false
    /// - is_change() returns true
    /// - is_exit() returns false
    #[test]
    fn test_typed_state_change_change() {
        let change: TypedStateChange<String> = TypedStateChange::Change("new_state".to_string());
        assert!(!change.is_keep());
        assert!(change.is_change());
        assert!(!change.is_exit());
    }

    /// # TypedStateChange Exit Variant
    ///
    /// Tests the Exit variant of TypedStateChange.
    ///
    /// ## Test Scenario
    /// - Creates a TypedStateChange::Exit
    /// - Checks the helper methods
    ///
    /// ## Expected Outcome
    /// - is_keep() returns false
    /// - is_change() returns false
    /// - is_exit() returns true
    #[test]
    fn test_typed_state_change_exit() {
        let change: TypedStateChange<String> = TypedStateChange::Exit;
        assert!(!change.is_keep());
        assert!(!change.is_change());
        assert!(change.is_exit());
    }

    /// # TypedStateChange Map Function
    ///
    /// Tests the map function for converting state types.
    ///
    /// ## Test Scenario
    /// - Creates various TypedStateChange variants
    /// - Maps them using a conversion function
    ///
    /// ## Expected Outcome
    /// - Keep maps to Keep
    /// - Exit maps to Exit
    /// - Change applies the conversion function
    #[test]
    fn test_typed_state_change_map() {
        // Test mapping Keep
        let keep: TypedStateChange<i32> = TypedStateChange::Keep;
        let mapped_keep: TypedStateChange<String> = keep.map(|n| n.to_string());
        assert!(mapped_keep.is_keep());

        // Test mapping Exit
        let exit: TypedStateChange<i32> = TypedStateChange::Exit;
        let mapped_exit: TypedStateChange<String> = exit.map(|n| n.to_string());
        assert!(mapped_exit.is_exit());

        // Test mapping Change
        let change: TypedStateChange<i32> = TypedStateChange::Change(42);
        let mapped_change: TypedStateChange<String> = change.map(|n| n.to_string());
        assert!(mapped_change.is_change());
        if let TypedStateChange::Change(s) = mapped_change {
            assert_eq!(s, "42");
        } else {
            panic!("Expected Change variant");
        }
    }

    /// # TypedStateChange Debug Implementation
    ///
    /// Tests that TypedStateChange implements Debug correctly.
    ///
    /// ## Test Scenario
    /// - Creates each variant
    /// - Formats them using Debug
    ///
    /// ## Expected Outcome
    /// - Should produce readable debug output
    #[test]
    fn test_typed_state_change_debug() {
        let keep: TypedStateChange<&str> = TypedStateChange::Keep;
        let change: TypedStateChange<&str> = TypedStateChange::Change("test");
        let exit: TypedStateChange<&str> = TypedStateChange::Exit;

        assert_eq!(format!("{:?}", keep), "Keep");
        assert_eq!(format!("{:?}", change), "Change(\"test\")");
        assert_eq!(format!("{:?}", exit), "Exit");
    }
}
