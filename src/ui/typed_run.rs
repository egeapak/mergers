//! Typed run loop implementations for mode-specific state machines.
//!
//! This module provides fully type-safe run loops that use [`TypedAppState`]
//! and mode-specific state enums instead of `Box<dyn AppState>`. This enables
//! compile-time verification of state transitions within each mode.
//!
//! # Benefits
//!
//! - **Compile-time type safety**: State transitions are verified at compile time
//! - **No runtime dispatch**: Direct method calls instead of virtual dispatch
//! - **Mode isolation**: Each mode has its own state machine
//!
//! # Example
//!
//! ```ignore
//! use mergers::ui::typed_run::run_merge_mode;
//!
//! let mut merge_app = MergeApp::new(config, client);
//! let initial_state = MergeState::initial();
//! run_merge_mode(&mut terminal, &mut merge_app, &event_source, initial_state).await?;
//! ```

use crate::ui::EventSource;
use crate::ui::apps::{CleanupApp, MergeApp, MigrationApp};
use crate::ui::state::typed::{TypedAppState, TypedStateChange};
use crate::ui::state::{CleanupModeState, MergeState, MigrationModeState};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;

/// Macro to process typed state changes and handle Keep/Change/Exit
macro_rules! handle_typed_state_change {
    ($result:expr, $current_state:expr) => {
        match $result {
            TypedStateChange::Keep => {}
            TypedStateChange::Change(new_state) => {
                $current_state = new_state;
            }
            TypedStateChange::Exit => break,
        }
    };
}

/// Run the merge mode application loop with typed state management.
///
/// This function provides a fully type-safe run loop for merge mode.
/// All state transitions are verified at compile time through the
/// [`TypedAppState`] trait and [`MergeState`] enum.
///
/// # Arguments
///
/// * `terminal` - The terminal to draw to
/// * `app` - The merge mode application state
/// * `event_source` - The source of terminal events
/// * `initial_state` - The initial state to start from
///
/// # Returns
///
/// `Ok(())` on clean exit, or an error if something fails.
pub async fn run_merge_mode<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut MergeApp,
    event_source: &dyn EventSource,
    initial_state: MergeState,
) -> anyhow::Result<()> {
    let mut current_state = initial_state;

    loop {
        terminal.draw(|f| TypedAppState::ui(&mut current_state, f, app))?;

        if event_source.poll(std::time::Duration::from_millis(50))? {
            match event_source.read()? {
                Event::Key(key) => {
                    handle_typed_state_change!(
                        TypedAppState::process_key(&mut current_state, key.code, app).await,
                        current_state
                    );
                }
                Event::Mouse(mouse) => {
                    handle_typed_state_change!(
                        TypedAppState::process_mouse(&mut current_state, mouse, app).await,
                        current_state
                    );
                }
                _ => {}
            }
        } else {
            handle_typed_state_change!(
                TypedAppState::process_key(&mut current_state, KeyCode::Null, app).await,
                current_state
            );
        }
    }

    Ok(())
}

/// Run the migration mode application loop with typed state management.
///
/// This function provides a fully type-safe run loop for migration mode.
/// All state transitions are verified at compile time through the
/// [`TypedAppState`] trait and [`MigrationModeState`] enum.
///
/// # Arguments
///
/// * `terminal` - The terminal to draw to
/// * `app` - The migration mode application state
/// * `event_source` - The source of terminal events
/// * `initial_state` - The initial state to start from
///
/// # Returns
///
/// `Ok(())` on clean exit, or an error if something fails.
pub async fn run_migration_mode<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut MigrationApp,
    event_source: &dyn EventSource,
    initial_state: MigrationModeState,
) -> anyhow::Result<()> {
    let mut current_state = initial_state;

    loop {
        terminal.draw(|f| TypedAppState::ui(&mut current_state, f, app))?;

        if event_source.poll(std::time::Duration::from_millis(50))? {
            match event_source.read()? {
                Event::Key(key) => {
                    handle_typed_state_change!(
                        TypedAppState::process_key(&mut current_state, key.code, app).await,
                        current_state
                    );
                }
                Event::Mouse(mouse) => {
                    handle_typed_state_change!(
                        TypedAppState::process_mouse(&mut current_state, mouse, app).await,
                        current_state
                    );
                }
                _ => {}
            }
        } else {
            handle_typed_state_change!(
                TypedAppState::process_key(&mut current_state, KeyCode::Null, app).await,
                current_state
            );
        }
    }

    Ok(())
}

/// Run the cleanup mode application loop with typed state management.
///
/// This function provides a fully type-safe run loop for cleanup mode.
/// All state transitions are verified at compile time through the
/// [`TypedAppState`] trait and [`CleanupModeState`] enum.
///
/// # Arguments
///
/// * `terminal` - The terminal to draw to
/// * `app` - The cleanup mode application state
/// * `event_source` - The source of terminal events
/// * `initial_state` - The initial state to start from
///
/// # Returns
///
/// `Ok(())` on clean exit, or an error if something fails.
pub async fn run_cleanup_mode<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut CleanupApp,
    event_source: &dyn EventSource,
    initial_state: CleanupModeState,
) -> anyhow::Result<()> {
    let mut current_state = initial_state;

    loop {
        terminal.draw(|f| TypedAppState::ui(&mut current_state, f, app))?;

        if event_source.poll(std::time::Duration::from_millis(50))? {
            match event_source.read()? {
                Event::Key(key) => {
                    handle_typed_state_change!(
                        TypedAppState::process_key(&mut current_state, key.code, app).await,
                        current_state
                    );
                }
                Event::Mouse(mouse) => {
                    handle_typed_state_change!(
                        TypedAppState::process_mouse(&mut current_state, mouse, app).await,
                        current_state
                    );
                }
                _ => {}
            }
        } else {
            handle_typed_state_change!(
                TypedAppState::process_key(&mut current_state, KeyCode::Null, app).await,
                current_state
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AzureDevOpsClient;
    use crate::models::{
        AppConfig, CleanupModeConfig, DefaultModeConfig, MigrationModeConfig, SharedConfig,
    };
    use crate::parsed_property::ParsedProperty;
    use crate::ui::MockEventSource;
    use crate::ui::state::ErrorState;
    use ratatui::backend::TestBackend;
    use std::sync::Arc;

    fn create_shared_config() -> SharedConfig {
        SharedConfig {
            organization: ParsedProperty::Default("test_org".to_string()),
            project: ParsedProperty::Default("test_project".to_string()),
            repository: ParsedProperty::Default("test_repo".to_string()),
            pat: ParsedProperty::Default("test_pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(4),
            max_concurrent_network: ParsedProperty::Default(10),
            max_concurrent_processing: ParsedProperty::Default(5),
            tag_prefix: ParsedProperty::Default("merged/".to_string()),
            since: None,
            skip_confirmation: false,
        }
    }

    fn create_test_client() -> AzureDevOpsClient {
        AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap()
    }

    /// # Typed Run Merge Mode Exit
    ///
    /// Tests that the typed merge mode run loop exits on 'q'.
    ///
    /// ## Test Scenario
    /// - Creates merge app and Error state
    /// - Sends 'q' key
    ///
    /// ## Expected Outcome
    /// - Loop exits cleanly
    #[tokio::test]
    async fn test_run_merge_mode_exit() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let config = Arc::new(AppConfig::Default {
            shared: create_shared_config(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        });
        let client = create_test_client();
        let mut app = MergeApp::new(config, client);

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let initial_state = MergeState::Error(ErrorState::new());
        let result = run_merge_mode(&mut terminal, &mut app, &events, initial_state).await;

        assert!(result.is_ok());
    }

    /// # Typed Run Migration Mode Exit
    ///
    /// Tests that the typed migration mode run loop exits on 'q'.
    ///
    /// ## Test Scenario
    /// - Creates migration app and Error state
    /// - Sends 'q' key
    ///
    /// ## Expected Outcome
    /// - Loop exits cleanly
    #[tokio::test]
    async fn test_run_migration_mode_exit() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let config = Arc::new(AppConfig::Migration {
            shared: create_shared_config(),
            migration: MigrationModeConfig {
                terminal_states: ParsedProperty::Default(vec!["Closed".to_string()]),
            },
        });
        let client = create_test_client();
        let mut app = MigrationApp::new(config, client);

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let initial_state = MigrationModeState::Error(ErrorState::new());
        let result = run_migration_mode(&mut terminal, &mut app, &events, initial_state).await;

        assert!(result.is_ok());
    }

    /// # Typed Run Cleanup Mode Exit
    ///
    /// Tests that the typed cleanup mode run loop exits on 'q'.
    ///
    /// ## Test Scenario
    /// - Creates cleanup app and Error state
    /// - Sends 'q' key
    ///
    /// ## Expected Outcome
    /// - Loop exits cleanly
    #[tokio::test]
    async fn test_run_cleanup_mode_exit() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let config = Arc::new(AppConfig::Cleanup {
            shared: create_shared_config(),
            cleanup: CleanupModeConfig {
                target: ParsedProperty::Default("main".to_string()),
            },
        });
        let client = create_test_client();
        let mut app = CleanupApp::new(config, client);

        let events = MockEventSource::new().with_key(KeyCode::Char('q'));

        let initial_state = CleanupModeState::Error(ErrorState::new());
        let result = run_cleanup_mode(&mut terminal, &mut app, &events, initial_state).await;

        assert!(result.is_ok());
    }

    /// # Typed Run Multiple Keys Before Exit
    ///
    /// Tests that multiple keys are processed before exit.
    ///
    /// ## Test Scenario
    /// - Creates merge app and Error state
    /// - Sends multiple keys then 'q'
    ///
    /// ## Expected Outcome
    /// - All keys processed, then exits
    #[tokio::test]
    async fn test_run_merge_mode_multiple_keys() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let config = Arc::new(AppConfig::Default {
            shared: create_shared_config(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        });
        let client = create_test_client();
        let mut app = MergeApp::new(config, client);

        let events = MockEventSource::new()
            .with_key(KeyCode::Down)
            .with_key(KeyCode::Up)
            .with_key(KeyCode::Enter)
            .with_key(KeyCode::Char('q'));

        let initial_state = MergeState::Error(ErrorState::new());
        let result = run_merge_mode(&mut terminal, &mut app, &events, initial_state).await;

        assert!(result.is_ok());
        assert!(events.is_empty());
    }
}
