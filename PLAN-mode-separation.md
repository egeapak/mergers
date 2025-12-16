# Plan: Mode-Specific App Types with Shared WorktreeContext

## Overview

Refactor the single `App` struct into three separate app types (`MergeApp`, `MigrationApp`, `CleanupApp`) with:
- A shared `WorktreeContext` for worktree-related state
- A common `AppMode` trait for shared behavior
- `Deref` implementations for ergonomic access to base fields

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

### 6. Updated AppState Trait

The `AppState` trait remains mostly unchanged, but now states receive correctly-typed apps:

```rust
// For merge mode states - they receive &mut MergeApp directly
// src/ui/state/default/cherry_pick.rs

#[async_trait]
impl AppState for CherryPickState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        // Use pattern matching when needed for mode-specific fields
        if let App::Merge(merge_app) = app {
            // Access merge_app.cherry_pick_items directly
            // Access merge_app.organization() via Deref
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        if let App::Merge(merge_app) = app {
            merge_app.cherry_pick_items.push(item);
            // merge_app.worktree.repo_path via Deref to base
        }
        StateChange::Keep
    }
}
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

### Accessing mode-specific fields

```rust
// Pattern match for type safety
if let App::Merge(merge_app) = app {
    merge_app.cherry_pick_items.push(item);
    merge_app.current_cherry_pick_index += 1;
}

if let App::Migration(migration_app) = app {
    migration_app.migration_analysis = Some(analysis);
    migration_app.mark_pr_as_eligible(123);
}

if let App::Cleanup(cleanup_app) = app {
    cleanup_app.cleanup_branches.push(branch);
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

### Phase 1: Create New Structures (non-breaking)

1. **Create `src/ui/worktree_context.rs`**
   - Define `WorktreeContext` struct
   - Implement `cleanup()` method
   - Implement `Drop` trait

2. **Create `src/ui/app_base.rs`**
   - Define `AppBase` struct with shared fields
   - Move configuration getter methods here
   - Move shared helper methods (open_pr_in_browser, etc.)

3. **Create `src/ui/app_mode.rs`**
   - Define `AppMode` trait

4. **Create `src/ui/apps/` directory**
   - `src/ui/apps/mod.rs` - exports
   - `src/ui/apps/merge_app.rs` - MergeApp with Deref + AppMode
   - `src/ui/apps/migration_app.rs` - MigrationApp with Deref + AppMode
   - `src/ui/apps/cleanup_app.rs` - CleanupApp with Deref + AppMode

### Phase 2: Refactor App

5. **Update `src/ui/app.rs`**
   - Change `App` from struct to enum
   - Implement `AppMode` trait via delegation
   - Implement `Deref`/`DerefMut` to AppBase
   - Add `from_config()` factory

6. **Update `src/ui/mod.rs`**
   - Export new modules

### Phase 3: Update State Modules

7. **Update Default/Merge states** (`src/ui/state/default/`)
   - Pattern match `App::Merge(merge_app)` for mode-specific fields
   - Shared fields accessible directly via Deref

8. **Update Migration states** (`src/ui/state/migration/`)
   - Pattern match `App::Migration(migration_app)` for mode-specific fields
   - Update worktree tracking: `app.worktree.worktree_id`

9. **Update Cleanup states** (`src/ui/state/cleanup/`)
   - Pattern match `App::Cleanup(cleanup_app)` for mode-specific fields

10. **Update Shared states** (`src/ui/state/shared/`)
    - Use shared fields directly via Deref

### Phase 4: Update Entry Points

11. **Update `src/ui/mod.rs` - `run_app_with_events()`**
    - Update to work with new App enum

12. **Update `src/bin/mergers.rs`**
    - Update App creation to use `App::from_config()`

### Phase 5: Cleanup and Tests

13. **Remove old code from App**
    - Remove mode-specific fields
    - Remove `cleanup_migration_worktree()` (now in WorktreeContext)
    - Remove Drop impl (now in WorktreeContext)

14. **Update tests**
    - Update test helpers in `src/ui/testing.rs`
    - Add tests for WorktreeContext
    - Add tests for AppMode trait
    - Update existing tests for new structure

## File Structure After Refactoring

```
src/ui/
├── mod.rs                    # Module exports + run_app_with_events
├── app.rs                    # App enum with Deref + AppMode delegation
├── app_base.rs               # AppBase struct (shared state)
├── app_mode.rs               # AppMode trait definition
├── worktree_context.rs       # WorktreeContext with Drop cleanup
├── apps/
│   ├── mod.rs                # Exports MergeApp, MigrationApp, CleanupApp
│   ├── merge_app.rs          # MergeApp struct
│   ├── migration_app.rs      # MigrationApp struct
│   └── cleanup_app.rs        # CleanupApp struct
├── state/
│   ├── mod.rs                # AppState trait
│   ├── default/              # Merge mode states
│   ├── migration/            # Migration mode states
│   ├── cleanup/              # Cleanup mode states
│   └── shared/               # Shared states
├── events/                   # Event handling
├── testing.rs                # Test helpers
└── snapshot_testing.rs       # Snapshot test utilities
```

## Benefits

1. **Type Safety**: Can't access `cherry_pick_items` in migration mode - compile error
2. **Ergonomic Access**: `app.organization()` works via Deref, no verbose accessors needed
3. **Clear Ownership**: WorktreeContext owns cleanup via Drop
4. **Extensibility**: Add new modes by implementing `AppMode` trait
5. **Trait-based Delegation**: App enum delegates to inner type, no manual routing
6. **Reduced Boilerplate**: Deref eliminates need for forwarding methods

## Migration Pattern for State Files

### Before (current):
```rust
async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
    app.cherry_pick_items.push(item);      // Direct access to wrong mode's field
    app.organization();                     // Method on App
    app.base_repo_path = Some(path);       // Direct field access
}
```

### After (new):
```rust
async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
    // Mode-specific: pattern match for compile-time safety
    if let App::Merge(merge_app) = app {
        merge_app.cherry_pick_items.push(item);
    }

    // Shared: direct access via Deref
    app.organization();                     // Works via Deref
    app.worktree.base_repo_path = Some(path);  // Via Deref to base.worktree
}
```

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Large refactor touching many files | Phase the implementation, keep tests passing |
| Pattern matching verbosity in states | States only need to match when accessing mode-specific fields |
| Deref can hide where data comes from | Clear documentation, consistent patterns |
