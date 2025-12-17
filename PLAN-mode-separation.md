# Plan: Mode-Specific App Types with Associated Type States

## Overview

Refactor the single `App` struct into three separate app types (`MergeApp`, `MigrationApp`, `CleanupApp`) with:
- A shared `WorktreeContext` for worktree-related state
- A common `AppMode` trait for shared behavior
- `Deref` implementations for ergonomic access to base fields
- **Associated types on `AppState` trait** for compile-time type safety
- **Mode-specific state enums** instead of `Box<dyn AppState>`
- **Generic shared state wrapper** for states used by multiple modes

## Current State

```rust
// Current: Single App with all mode-specific fields mixed
pub struct App {
    // Shared
    pub config: Arc<AppConfig>,
    pub pull_requests: Vec<PullRequestWithWorkItems>,
    pub client: AzureDevOpsClient,
    pub version: Option<String>,
    pub repo_path: Option<PathBuf>,
    pub base_repo_path: Option<PathBuf>,
    pub _temp_dir: Option<TempDir>,
    pub error_message: Option<String>,
    pub initial_state: Option<Box<dyn AppState>>,

    // Merge mode only
    pub cherry_pick_items: Vec<CherryPickItem>,
    pub current_cherry_pick_index: usize,

    // Migration mode only
    pub migration_analysis: Option<MigrationAnalysis>,
    pub migration_worktree_id: Option<String>,

    // Cleanup mode only
    pub cleanup_branches: Vec<CleanupBranch>,
}
```

## Proposed Architecture

### 1. Shared WorktreeContext

```rust
// src/ui/worktree_context.rs

/// Shared context for worktree management across modes
pub struct WorktreeContext {
    /// Path to the working repository (worktree or cloned repo)
    pub repo_path: Option<PathBuf>,
    /// Base repository path (for worktree cleanup)
    pub base_repo_path: Option<PathBuf>,
    /// Temporary directory handle (keeps cloned repos alive)
    pub _temp_dir: Option<TempDir>,
    /// Worktree ID for cleanup (e.g., "migration-123456" or "1.0.0")
    pub worktree_id: Option<String>,
}

impl WorktreeContext {
    pub fn new() -> Self { ... }

    /// Clean up the worktree if one was created
    pub fn cleanup(&mut self) {
        if let (Some(base_repo), Some(worktree_id)) =
            (&self.base_repo_path, self.worktree_id.take())
        {
            let _ = crate::git::force_remove_worktree(base_repo, &worktree_id);
        }
    }
}

impl Drop for WorktreeContext {
    fn drop(&mut self) {
        self.cleanup();
    }
}
```

### 2. Shared Base State

```rust
// src/ui/app_base.rs

/// Shared state common to all app modes
pub struct AppBase {
    pub config: Arc<AppConfig>,
    pub pull_requests: Vec<PullRequestWithWorkItems>,
    pub client: AzureDevOpsClient,
    pub version: Option<String>,
    pub worktree: WorktreeContext,
    pub error_message: Option<String>,
}

impl AppBase {
    pub fn new(config: Arc<AppConfig>, client: AzureDevOpsClient) -> Self { ... }

    // Configuration getters
    pub fn organization(&self) -> &str { self.config.shared().organization.value() }
    pub fn project(&self) -> &str { self.config.shared().project.value() }
    pub fn repository(&self) -> &str { self.config.shared().repository.value() }
    pub fn dev_branch(&self) -> &str { self.config.shared().dev_branch.value() }
    pub fn target_branch(&self) -> &str { self.config.shared().target_branch.value() }
    pub fn local_repo(&self) -> Option<&str> { ... }
    pub fn max_concurrent_network(&self) -> usize { ... }
    pub fn max_concurrent_processing(&self) -> usize { ... }
    pub fn tag_prefix(&self) -> &str { ... }
    pub fn since(&self) -> Option<&str> { ... }

    // Shared helpers
    pub fn get_selected_prs(&self) -> Vec<&PullRequestWithWorkItems> { ... }
    pub fn open_pr_in_browser(&self, pr_id: i32) { ... }
    pub fn open_work_items_in_browser(&self, work_items: &[WorkItem]) { ... }
}
```

### 3. AppMode Trait

```rust
// src/ui/app_mode.rs

use async_trait::async_trait;

/// Common trait for all app mode types
///
/// This trait defines shared behavior that all app modes must implement,
/// allowing the App enum to delegate to mode-specific implementations.
#[async_trait]
pub trait AppMode: Send + Sync {
    /// Get a reference to the shared base state
    fn base(&self) -> &AppBase;

    /// Get a mutable reference to the shared base state
    fn base_mut(&mut self) -> &mut AppBase;

    /// Get the initial state for this app mode
    fn take_initial_state(&mut self) -> Option<Box<dyn AppState>>;

    /// Set the initial state for this app mode
    fn set_initial_state(&mut self, state: Box<dyn AppState>);
}
```

### 4. Mode-Specific App Types with Deref

```rust
// src/ui/apps/merge_app.rs

pub struct MergeApp {
    base: AppBase,
    pub cherry_pick_items: Vec<CherryPickItem>,
    pub current_cherry_pick_index: usize,
    initial_state: Option<Box<dyn AppState>>,
}

impl MergeApp {
    pub fn new(config: Arc<AppConfig>, client: AzureDevOpsClient) -> Self { ... }

    // Merge-specific methods
    pub fn work_item_state(&self) -> &str {
        match self.config.as_ref() {
            AppConfig::Default { default, .. } => default.work_item_state.value(),
            _ => "Next Merged", // fallback
        }
    }
}

impl Deref for MergeApp {
    type Target = AppBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for MergeApp {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

#[async_trait]
impl AppMode for MergeApp {
    fn base(&self) -> &AppBase { &self.base }
    fn base_mut(&mut self) -> &mut AppBase { &mut self.base }
    fn take_initial_state(&mut self) -> Option<Box<dyn AppState>> { self.initial_state.take() }
    fn set_initial_state(&mut self, state: Box<dyn AppState>) { self.initial_state = Some(state); }
}
```

```rust
// src/ui/apps/migration_app.rs

pub struct MigrationApp {
    base: AppBase,
    pub migration_analysis: Option<MigrationAnalysis>,
    initial_state: Option<Box<dyn AppState>>,
}

impl MigrationApp {
    pub fn new(config: Arc<AppConfig>, client: AzureDevOpsClient) -> Self { ... }

    // Migration-specific methods
    pub fn terminal_states(&self) -> &[String] {
        match self.config.as_ref() {
            AppConfig::Migration { migration, .. } => migration.terminal_states.value(),
            _ => &[], // fallback
        }
    }

    // Manual override methods (moved from App)
    pub fn mark_pr_as_eligible(&mut self, pr_id: i32) { ... }
    pub fn mark_pr_as_not_eligible(&mut self, pr_id: i32) { ... }
    pub fn remove_manual_override(&mut self, pr_id: i32) { ... }
    pub fn has_manual_override(&self, pr_id: i32) -> Option<bool> { ... }
}

impl Deref for MigrationApp {
    type Target = AppBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for MigrationApp {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

#[async_trait]
impl AppMode for MigrationApp {
    fn base(&self) -> &AppBase { &self.base }
    fn base_mut(&mut self) -> &mut AppBase { &mut self.base }
    fn take_initial_state(&mut self) -> Option<Box<dyn AppState>> { self.initial_state.take() }
    fn set_initial_state(&mut self, state: Box<dyn AppState>) { self.initial_state = Some(state); }
}
```

```rust
// src/ui/apps/cleanup_app.rs

pub struct CleanupApp {
    base: AppBase,
    pub cleanup_branches: Vec<CleanupBranch>,
    initial_state: Option<Box<dyn AppState>>,
}

impl CleanupApp {
    pub fn new(config: Arc<AppConfig>, client: AzureDevOpsClient) -> Self { ... }

    // Cleanup-specific methods
    pub fn cleanup_target(&self) -> &str {
        match self.config.as_ref() {
            AppConfig::Cleanup { cleanup, .. } => cleanup.target.value(),
            _ => self.target_branch(), // fallback
        }
    }
}

impl Deref for CleanupApp {
    type Target = AppBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for CleanupApp {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

#[async_trait]
impl AppMode for CleanupApp {
    fn base(&self) -> &AppBase { &self.base }
    fn base_mut(&mut self) -> &mut AppBase { &mut self.base }
    fn take_initial_state(&mut self) -> Option<Box<dyn AppState>> { self.initial_state.take() }
    fn set_initial_state(&mut self, state: Box<dyn AppState>) { self.initial_state = Some(state); }
}
```

### 5. App Enum with Trait Delegation

```rust
// src/ui/app.rs

pub enum App {
    Merge(MergeApp),
    Migration(MigrationApp),
    Cleanup(CleanupApp),
}

impl App {
    /// Create the appropriate app type based on config
    pub fn from_config(config: Arc<AppConfig>, client: AzureDevOpsClient) -> Self {
        match config.as_ref() {
            AppConfig::Default { .. } => App::Merge(MergeApp::new(config, client)),
            AppConfig::Migration { .. } => App::Migration(MigrationApp::new(config, client)),
            AppConfig::Cleanup { .. } => App::Cleanup(CleanupApp::new(config, client)),
        }
    }

    /// Get the inner app as a trait object
    fn as_mode(&self) -> &dyn AppMode {
        match self {
            App::Merge(app) => app,
            App::Migration(app) => app,
            App::Cleanup(app) => app,
        }
    }

    fn as_mode_mut(&mut self) -> &mut dyn AppMode {
        match self {
            App::Merge(app) => app,
            App::Migration(app) => app,
            App::Cleanup(app) => app,
        }
    }
}

// Delegate AppMode trait to inner type
#[async_trait]
impl AppMode for App {
    fn base(&self) -> &AppBase {
        self.as_mode().base()
    }

    fn base_mut(&mut self) -> &mut AppBase {
        self.as_mode_mut().base_mut()
    }

    fn take_initial_state(&mut self) -> Option<Box<dyn AppState>> {
        self.as_mode_mut().take_initial_state()
    }

    fn set_initial_state(&mut self, state: Box<dyn AppState>) {
        self.as_mode_mut().set_initial_state(state);
    }
}

// Deref to AppBase for ergonomic access: app.organization(), app.client, etc.
impl Deref for App {
    type Target = AppBase;
    fn deref(&self) -> &Self::Target {
        self.as_mode().base()
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mode_mut().base_mut()
    }
}
```

### 6. AppState Trait with Associated Type

The `AppState` trait uses an associated type to specify which app mode it works with. This provides compile-time type safety - states receive correctly-typed apps without pattern matching.

```rust
// src/ui/state/mod.rs

use async_trait::async_trait;

/// State change returned from state operations
pub enum StateChange<S> {
    /// Keep current state
    Keep,
    /// Change to a new state
    Change(S),
    /// Exit the application
    Exit,
}

/// Trait for all UI states
///
/// The associated type `App` specifies which app mode this state works with.
/// This enables compile-time type checking - a MergeState can only receive MergeApp.
#[async_trait]
pub trait AppState: Send + Sync {
    /// The app type this state works with
    type App: AppMode;

    /// Render the state's UI
    fn ui(&mut self, f: &mut Frame, app: &Self::App);

    /// Process keyboard input
    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut Self::App
    ) -> StateChange<Box<dyn AppState<App = Self::App>>>;

    /// Process mouse input (default: no-op)
    async fn process_mouse(
        &mut self,
        _event: MouseEvent,
        _app: &mut Self::App
    ) -> StateChange<Box<dyn AppState<App = Self::App>>> {
        StateChange::Keep
    }

    /// Get this state's name for logging/debugging
    fn name(&self) -> &'static str;
}

// Note: The existing `create_initial_state` factory function will be replaced
// by mode-specific initial state creation in the run functions.
```

### 7. Mode-Specific State Enums

Instead of `Box<dyn AppState>`, each mode has its own state enum. This provides better type safety and eliminates the need for trait objects in most cases.

```rust
// src/ui/state/default/mod.rs

/// All possible states for merge/default mode
pub enum MergeState {
    DataLoading(DataLoadingState),
    PullRequestSelection(PullRequestSelectionState),
    CherryPick(CherryPickState),
    Settings(SettingsState<MergeApp>),
    SettingsConfirmation(SettingsConfirmationState<MergeApp>),
    // ... other merge states
}

impl MergeState {
    pub fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        match self {
            MergeState::DataLoading(s) => s.ui(f, app),
            MergeState::PullRequestSelection(s) => s.ui(f, app),
            MergeState::CherryPick(s) => s.ui(f, app),
            MergeState::Settings(s) => s.ui(f, app),
            MergeState::SettingsConfirmation(s) => s.ui(f, app),
        }
    }

    pub async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MergeApp
    ) -> StateChange<MergeState> {
        match self {
            MergeState::DataLoading(s) => s.process_key(code, app).await,
            MergeState::PullRequestSelection(s) => s.process_key(code, app).await,
            MergeState::CherryPick(s) => s.process_key(code, app).await,
            MergeState::Settings(s) => s.process_key(code, app).await,
            MergeState::SettingsConfirmation(s) => s.process_key(code, app).await,
        }
    }

    pub async fn process_mouse(
        &mut self,
        event: MouseEvent,
        app: &mut MergeApp
    ) -> StateChange<MergeState> {
        match self {
            MergeState::DataLoading(s) => s.process_mouse(event, app).await,
            MergeState::PullRequestSelection(s) => s.process_mouse(event, app).await,
            MergeState::CherryPick(s) => s.process_mouse(event, app).await,
            MergeState::Settings(s) => s.process_mouse(event, app).await,
            MergeState::SettingsConfirmation(s) => s.process_mouse(event, app).await,
        }
    }
}
```

```rust
// src/ui/state/migration/mod.rs

/// All possible states for migration mode
pub enum MigrationState {
    DataLoading(MigrationDataLoadingState),
    Selection(MigrationSelectionState),
    Analysis(MigrationAnalysisState),
    Details(MigrationDetailsState),
    Settings(SettingsState<MigrationApp>),
    SettingsConfirmation(SettingsConfirmationState<MigrationApp>),
    // ... other migration states
}

// Similar impl as MergeState
```

```rust
// src/ui/state/cleanup/mod.rs

/// All possible states for cleanup mode
pub enum CleanupState {
    DataLoading(CleanupDataLoadingState),
    Selection(CleanupSelectionState),
    Confirmation(CleanupConfirmationState),
    Settings(SettingsState<CleanupApp>),
    SettingsConfirmation(SettingsConfirmationState<CleanupApp>),
    // ... other cleanup states
}

// Similar impl as MergeState
```

### 8. Generic Shared State Wrapper

States that are shared across modes (like `SettingsState`, `SettingsConfirmationState`) are made generic over the app type:

```rust
// src/ui/state/shared/settings.rs

/// Settings state - shared across all modes
///
/// Generic over app type to work with any mode while maintaining type safety.
pub struct SettingsState<A: AppMode> {
    config_display: ConfigDisplayData,
    scroll_offset: usize,
    _phantom: PhantomData<A>,
}

impl<A: AppMode> SettingsState<A> {
    pub fn new() -> Self {
        Self {
            config_display: ConfigDisplayData::default(),
            scroll_offset: 0,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<A: AppMode + Send + Sync> AppState for SettingsState<A> {
    type App = A;

    fn ui(&mut self, f: &mut Frame, app: &A) {
        // Access shared fields via Deref
        let config = &app.config;
        // ... render settings UI
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut A
    ) -> StateChange<Box<dyn AppState<App = A>>> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => StateChange::Exit,
            KeyCode::Enter => {
                // Return to previous state
                StateChange::Change(Box::new(SettingsConfirmationState::<A>::new()))
            }
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str { "Settings" }
}
```

```rust
// src/ui/state/shared/settings_confirmation.rs

/// Settings confirmation state - shared across all modes
pub struct SettingsConfirmationState<A: AppMode> {
    selected_option: usize,
    _phantom: PhantomData<A>,
}

impl<A: AppMode> SettingsConfirmationState<A> {
    pub fn new() -> Self {
        Self {
            selected_option: 0,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<A: AppMode + Send + Sync> AppState for SettingsConfirmationState<A> {
    type App = A;

    fn ui(&mut self, f: &mut Frame, app: &A) {
        // Render confirmation dialog
        // Access app.organization() via Deref if needed
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut A
    ) -> StateChange<Box<dyn AppState<App = A>>> {
        // Handle confirmation logic
        StateChange::Keep
    }

    fn name(&self) -> &'static str { "SettingsConfirmation" }
}
```

### 9. Root-Level Run Loop with Unwrapping

The main run loop handles unwrapping the App enum and dispatching to the correct state enum:

```rust
// src/ui/mod.rs

pub async fn run_app_with_events(
    mut app: App,
    events: &mut EventSource,
) -> Result<()> {
    match app {
        App::Merge(ref mut merge_app) => {
            run_merge_mode(merge_app, events).await
        }
        App::Migration(ref mut migration_app) => {
            run_migration_mode(migration_app, events).await
        }
        App::Cleanup(ref mut cleanup_app) => {
            run_cleanup_mode(cleanup_app, events).await
        }
    }
}

async fn run_merge_mode(
    app: &mut MergeApp,
    events: &mut EventSource,
) -> Result<()> {
    let mut state = MergeState::DataLoading(DataLoadingState::new());
    let mut terminal = setup_terminal()?;

    loop {
        terminal.draw(|f| state.ui(f, app))?;

        if let Some(event) = events.next().await {
            if let Event::Key(key) = event {
                match state.process_key(key.code, app).await {
                    StateChange::Keep => {}
                    StateChange::Change(new_state) => state = new_state,
                    StateChange::Exit => break,
                }
            }
        }
    }

    restore_terminal(terminal)?;
    Ok(())
}

// Similar run_migration_mode and run_cleanup_mode functions
```

## Usage Examples

### Accessing shared fields (via Deref)

```rust
// All of these work on App, MergeApp, MigrationApp, or CleanupApp
app.organization()           // via Deref -> AppBase
app.client.fetch_prs()       // via Deref -> AppBase
app.worktree.repo_path       // via Deref -> AppBase
app.pull_requests.len()      // via Deref -> AppBase
```

### Mode-specific states with correctly typed app

```rust
// In CherryPickState - receives MergeApp directly, no pattern matching needed
#[async_trait]
impl AppState for CherryPickState {
    type App = MergeApp;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        // Direct access to merge-specific fields
        let items = &app.cherry_pick_items;
        let index = app.current_cherry_pick_index;

        // Shared fields via Deref
        let org = app.organization();
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MergeApp
    ) -> StateChange<MergeState> {
        // Direct mutation of merge-specific fields
        app.cherry_pick_items.push(item);
        app.current_cherry_pick_index += 1;
        StateChange::Keep
    }

    fn name(&self) -> &'static str { "CherryPick" }
}
```

```rust
// In MigrationSelectionState - receives MigrationApp directly
#[async_trait]
impl AppState for MigrationSelectionState {
    type App = MigrationApp;

    fn ui(&mut self, f: &mut Frame, app: &MigrationApp) {
        // Direct access to migration-specific fields
        if let Some(analysis) = &app.migration_analysis {
            // render analysis...
        }
    }

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MigrationApp
    ) -> StateChange<MigrationState> {
        app.mark_pr_as_eligible(123);
        StateChange::Keep
    }

    fn name(&self) -> &'static str { "MigrationSelection" }
}
```

### Generic shared states

```rust
// SettingsState works with any app mode
let settings_state: SettingsState<MergeApp> = SettingsState::new();
let settings_state: SettingsState<MigrationApp> = SettingsState::new();

// State enums include the specialized version
enum MergeState {
    Settings(SettingsState<MergeApp>),
    // ...
}

enum MigrationState {
    Settings(SettingsState<MigrationApp>),
    // ...
}
```

### Worktree management

```rust
// Set up worktree tracking
app.worktree.base_repo_path = Some(path);
app.worktree.worktree_id = Some("migration-123".to_string());

// Cleanup happens automatically on drop, or manually:
app.worktree.cleanup();
```

## Implementation Steps

### Phase 1: Create Core Infrastructure (non-breaking)

1. **Create `src/ui/worktree_context.rs`**
   - Define `WorktreeContext` struct
   - Implement `cleanup()` method
   - Implement `Drop` trait
   - Add unit tests

2. **Create `src/ui/app_base.rs`**
   - Define `AppBase` struct with shared fields
   - Move configuration getter methods here
   - Move shared helper methods (open_pr_in_browser, etc.)
   - Add unit tests

3. **Create `src/ui/app_mode.rs`**
   - Define `AppMode` trait with `base()`, `base_mut()` methods
   - Add associated type bounds

### Phase 2: Create Mode-Specific App Types

4. **Create `src/ui/apps/` directory**
   - `src/ui/apps/mod.rs` - exports
   - `src/ui/apps/merge_app.rs` - MergeApp with Deref + AppMode
   - `src/ui/apps/migration_app.rs` - MigrationApp with Deref + AppMode
   - `src/ui/apps/cleanup_app.rs` - CleanupApp with Deref + AppMode

5. **Update `src/ui/app.rs`**
   - Change `App` from struct to enum
   - Implement `Deref`/`DerefMut` to AppBase
   - Add `from_config()` factory

6. **Update `src/ui/mod.rs`**
   - Export new modules

### Phase 3: Create State Infrastructure

7. **Update `src/ui/state/mod.rs`**
   - Update `AppState` trait with associated type `type App: AppMode`
   - Update `StateChange<S>` enum to be generic

8. **Create Mode-Specific State Enums**
   - `src/ui/state/default/mod.rs` - Add `MergeState` enum
   - `src/ui/state/migration/mod.rs` - Add `MigrationState` enum
   - `src/ui/state/cleanup/mod.rs` - Add `CleanupState` enum

9. **Update Shared States to be Generic**
   - `src/ui/state/shared/settings.rs` - `SettingsState<A: AppMode>`
   - `src/ui/state/shared/settings_confirmation.rs` - `SettingsConfirmationState<A: AppMode>`
   - Other shared states as needed

### Phase 4: Update Mode-Specific States

10. **Update Default/Merge states** (`src/ui/state/default/`)
    - Update `AppState` impl with `type App = MergeApp`
    - Change method signatures to receive `&MergeApp` / `&mut MergeApp`
    - Return `StateChange<MergeState>` from `process_key`
    - Direct access to `app.cherry_pick_items`, etc.

11. **Update Migration states** (`src/ui/state/migration/`)
    - Update `AppState` impl with `type App = MigrationApp`
    - Change method signatures to receive `&MigrationApp` / `&mut MigrationApp`
    - Return `StateChange<MigrationState>` from `process_key`
    - Direct access to `app.migration_analysis`, etc.
    - Update worktree tracking: `app.worktree.worktree_id`

12. **Update Cleanup states** (`src/ui/state/cleanup/`)
    - Update `AppState` impl with `type App = CleanupApp`
    - Change method signatures to receive `&CleanupApp` / `&mut CleanupApp`
    - Return `StateChange<CleanupState>` from `process_key`
    - Direct access to `app.cleanup_branches`, etc.

### Phase 5: Update Run Loop and Entry Points

13. **Update `src/ui/mod.rs` - `run_app_with_events()`**
    - Add root-level unwrapping: `match app { App::Merge(..) => run_merge_mode(), ... }`
    - Create mode-specific run functions: `run_merge_mode()`, `run_migration_mode()`, `run_cleanup_mode()`
    - Each run function works with mode's state enum directly

14. **Update `src/bin/mergers.rs`**
    - Update App creation to use `App::from_config()`

### Phase 6: Cleanup and Tests

15. **Remove old code from App**
    - Remove mode-specific fields (now in individual apps)
    - Remove `cleanup_migration_worktree()` (now in WorktreeContext)
    - Remove Drop impl (now in WorktreeContext)
    - Remove `initial_state` field (now managed by state enums)

16. **Update tests**
    - Update test helpers in `src/ui/testing.rs`
    - Add tests for WorktreeContext
    - Add tests for mode-specific apps
    - Update snapshot tests to use mode-specific apps
    - Update existing tests for new structure

## File Structure After Refactoring

```
src/ui/
├── mod.rs                    # Module exports + run_app_with_events + mode run functions
├── app.rs                    # App enum with Deref
├── app_base.rs               # AppBase struct (shared state)
├── app_mode.rs               # AppMode trait definition
├── worktree_context.rs       # WorktreeContext with Drop cleanup
├── apps/
│   ├── mod.rs                # Exports MergeApp, MigrationApp, CleanupApp
│   ├── merge_app.rs          # MergeApp struct
│   ├── migration_app.rs      # MigrationApp struct
│   └── cleanup_app.rs        # CleanupApp struct
├── state/
│   ├── mod.rs                # AppState trait with associated type
│   ├── default/
│   │   ├── mod.rs            # MergeState enum + merge state exports
│   │   ├── data_loading.rs   # DataLoadingState (type App = MergeApp)
│   │   ├── pr_selection.rs   # PRSelectionState (type App = MergeApp)
│   │   └── ...               # Other merge states
│   ├── migration/
│   │   ├── mod.rs            # MigrationState enum + migration state exports
│   │   ├── data_loading.rs   # MigrationDataLoadingState (type App = MigrationApp)
│   │   ├── selection.rs      # MigrationSelectionState (type App = MigrationApp)
│   │   └── ...               # Other migration states
│   ├── cleanup/
│   │   ├── mod.rs            # CleanupState enum + cleanup state exports
│   │   └── ...               # Cleanup states
│   └── shared/
│       ├── mod.rs            # Shared state exports
│       ├── settings.rs       # SettingsState<A: AppMode>
│       └── settings_confirmation.rs  # SettingsConfirmationState<A: AppMode>
├── events/                   # Event handling
├── testing.rs                # Test helpers
└── snapshot_testing.rs       # Snapshot test utilities
```

## Benefits

1. **Compile-Time Type Safety**: States declare `type App = MergeApp` - compiler ensures they receive correct app type
2. **No Pattern Matching in States**: States receive correctly typed app directly, no `if let App::Merge(app)` needed
3. **Mode-Specific State Enums**: `MergeState`, `MigrationState`, `CleanupState` replace `Box<dyn AppState>`
4. **Generic Shared States**: `SettingsState<A>` works with any mode while maintaining type safety
5. **Single Unwrapping Point**: Pattern matching on App enum happens once at root level in `run_app_with_events`
6. **Ergonomic Access**: `app.organization()` works via Deref to AppBase
7. **Clear Ownership**: WorktreeContext owns worktree cleanup via Drop
8. **Extensibility**: Add new modes by implementing `AppMode` trait + creating state enum
9. **Reduced Boilerplate**: Deref eliminates need for forwarding methods

## Migration Pattern for State Files

### Before (current):
```rust
// Current: States receive generic App, can access wrong mode's fields
#[async_trait]
impl AppState for CherryPickState {
    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        app.cherry_pick_items.push(item);      // Can access wrong mode's field
        app.migration_analysis = None;          // No compile-time protection!
        app.organization();                     // Method on App
        StateChange::Keep
    }
}
```

### After (new):
```rust
// New: States declare their app type via associated type
#[async_trait]
impl AppState for CherryPickState {
    type App = MergeApp;  // <-- Declares this state works with MergeApp

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MergeApp  // <-- Receives correctly typed app
    ) -> StateChange<MergeState> {
        // Direct access to mode-specific fields
        app.cherry_pick_items.push(item);
        app.current_cherry_pick_index += 1;

        // Shared fields via Deref - works automatically
        app.organization();                         // Via Deref
        app.worktree.base_repo_path = Some(path);  // Via Deref

        // app.migration_analysis = None;  // <-- COMPILE ERROR! Not on MergeApp

        StateChange::Change(MergeState::Selection(SelectionState::new()))
    }
}
```

### For shared states:
```rust
// Generic over app type
#[async_trait]
impl<A: AppMode + Send + Sync> AppState for SettingsState<A> {
    type App = A;  // <-- Works with any app mode

    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut A
    ) -> StateChange<Box<dyn AppState<App = A>>> {
        // Only access shared fields via Deref
        let org = app.organization();
        StateChange::Keep
    }
}
```

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Large refactor touching many files | Phase the implementation, keep tests passing after each phase |
| State enum maintenance overhead | Each mode has its own enum, but changes are isolated per mode |
| Generic shared states complexity | PhantomData pattern is well-established; limited to truly shared states |
| Deref can hide where data comes from | Clear documentation, consistent patterns, IDE support helps |
| Associated type complexity | Provides compile-time safety that pays off in maintenance |
| Box<dyn AppState> still needed for shared states | Only used in shared states, mode-specific states use concrete enums |

## Design Decisions

### Why Associated Types over Generic Parameters?

Associated types (`type App: AppMode`) vs generic parameters (`AppState<A: AppMode>`) trade-offs:

- **Associated types**: Each state struct declares ONE app type it works with
- **Generic parameters**: Would require `CherryPickState<A>` even though it only works with MergeApp
- **Chosen**: Associated types because states have a fixed relationship with their app mode

### Why State Enums instead of Trait Objects?

| Aspect | `Box<dyn AppState>` | Mode-specific enums |
|--------|---------------------|---------------------|
| Type safety | Runtime dispatch | Compile-time |
| Performance | Virtual call overhead | Direct dispatch |
| Maintenance | One change point | Update enum per new state |
| Extensibility | Easy to add states | Must update enum |

**Chosen**: Mode-specific enums for type safety. The number of states per mode is stable and bounded.

### Why Keep Box<dyn AppState> for Shared States?

Shared states like `SettingsState<A>` need to return different state types based on context:
- In MergeState: might return `MergeState::Selection`
- In MigrationState: might return `MigrationState::Selection`

Rather than creating complex return type abstractions, we use `Box<dyn AppState<App = A>>` for these specific cases. This is isolated to shared states only.

## State Transition Examples

### Mode-specific state transitions
```rust
// In MergeState enum
StateChange::Change(MergeState::CherryPick(CherryPickState::new()))
StateChange::Change(MergeState::Selection(SelectionState::new()))

// In MigrationState enum
StateChange::Change(MigrationState::Analysis(AnalysisState::new()))
StateChange::Change(MigrationState::Details(DetailsState::new()))
```

### Transitions from shared states
```rust
// In SettingsState<MergeApp>::process_key
// Returns to caller which handles the transition
StateChange::Exit  // Let the run loop handle returning to previous state

// Or maintain previous state in the shared state struct
pub struct SettingsState<A: AppMode> {
    previous_state: Option<...>,  // If needed
    _phantom: PhantomData<A>,
}
```