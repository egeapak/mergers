//! App mode trait for shared behavior across app types.
//!
//! This module defines the [`AppMode`] trait which all mode-specific
//! app types (MergeApp, MigrationApp, CleanupApp) implement.
//!
//! # Type Safety
//!
//! The `AppMode` trait uses an associated `Config` type to ensure
//! compile-time type safety between apps and their configurations:
//!
//! - `MergeApp` has `type Config = MergeConfig`
//! - `MigrationApp` has `type Config = MigrationConfig`
//! - `CleanupApp` has `type Config = CleanupConfig`

use crate::models::AppModeConfig;
use crate::ui::AppBase;
use std::sync::Arc;

/// Trait for all app mode types with compile-time config type safety.
///
/// This trait defines shared behavior that all app modes must implement,
/// providing access to the common [`AppBase`] state. Mode-specific apps
/// implement this trait along with `Deref`/`DerefMut` to AppBase for
/// ergonomic field access.
///
/// # Associated Type
///
/// The `Config` associated type must implement [`AppModeConfig`], which
/// ensures type-safe access to both shared and mode-specific configuration.
/// This eliminates the need for runtime pattern matching on config variants.
///
/// # Example
///
/// ```ignore
/// use crate::models::MergeConfig;
///
/// impl AppMode for MergeApp {
///     type Config = MergeConfig;
///
///     fn base(&self) -> &AppBase<MergeConfig> { &self.base }
///     fn base_mut(&mut self) -> &mut AppBase<MergeConfig> { &mut self.base }
/// }
///
/// fn process_app<A: AppMode>(app: &A) {
///     // Access shared config through the trait
///     let org = app.config().shared().organization.value();
///     let prs = &app.base().pull_requests;
/// }
/// ```
pub trait AppMode: Send + Sync {
    /// The configuration type this app mode uses.
    ///
    /// Must implement [`AppModeConfig`] to ensure access to shared configuration.
    type Config: AppModeConfig + Send + Sync;

    /// Returns a reference to the shared base state.
    fn base(&self) -> &AppBase<Self::Config>;

    /// Returns a mutable reference to the shared base state.
    fn base_mut(&mut self) -> &mut AppBase<Self::Config>;

    /// Returns a reference to the configuration.
    ///
    /// This is a convenience method that provides direct access to the
    /// type-safe configuration without going through `base()`.
    fn config(&self) -> &Arc<Self::Config> {
        &self.base().config
    }
}
