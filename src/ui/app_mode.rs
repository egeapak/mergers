//! App mode trait for shared behavior across app types.
//!
//! This module defines the [`AppMode`] trait which all mode-specific
//! app types (MergeApp, MigrationApp, CleanupApp) implement.

use crate::ui::AppBase;

/// Trait for all app mode types.
///
/// This trait defines shared behavior that all app modes must implement,
/// providing access to the common [`AppBase`] state. Mode-specific apps
/// implement this trait along with `Deref`/`DerefMut` to AppBase for
/// ergonomic field access.
///
/// # Example
///
/// ```ignore
/// fn process_shared_state<A: AppMode>(app: &A) {
///     // Access AppBase through the trait
///     let org = app.base().organization();
///     let prs = &app.base().pull_requests;
/// }
/// ```
pub trait AppMode: Send + Sync {
    /// Returns a reference to the shared base state.
    fn base(&self) -> &AppBase;

    /// Returns a mutable reference to the shared base state.
    fn base_mut(&mut self) -> &mut AppBase;
}
