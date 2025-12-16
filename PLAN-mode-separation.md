# Plan: Mode-Specific App Types with Shared WorktreeContext

## Overview

Refactor the single `App` struct into three separate app types (`MergeApp`, `MigrationApp`, `CleanupApp`) with a shared `WorktreeContext` for worktree-related state.

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
    pub base_repo_path: Option<PathBuf>,  // Used by multiple modes
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
// src/ui/worktree_context.rs (new file)

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
    pub fn new() -> Self {
        Self {
            repo_path: None,
            base_repo_path: None,
            _temp_dir: None,
            worktree_id: None,
        }
    }

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
// src/ui/app_base.rs (new file)

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
    pub fn new(
        config: Arc<AppConfig>,
        client: AzureDevOpsClient,
    ) -> Self {
        Self {
            config,
            pull_requests: Vec::new(),
            client,
            version: None,
            worktree: WorktreeContext::new(),
            error_message: None,
        }
    }

    // Configuration getters (moved from App)
    pub fn organization(&self) -> &str { ... }
    pub fn project(&self) -> &str { ... }
    pub fn repository(&self) -> &str { ... }
    // ... etc
}
```

### 3. Mode-Specific App Types

```rust
// src/ui/apps/merge_app.rs
pub struct MergeApp {
    pub base: AppBase,
    pub cherry_pick_items: Vec<CherryPickItem>,
    pub current_cherry_pick_index: usize,
    pub initial_state: Option<Box<dyn AppState>>,
}

// src/ui/apps/migration_app.rs
pub struct MigrationApp {
    pub base: AppBase,
    pub migration_analysis: Option<MigrationAnalysis>,
    pub initial_state: Option<Box<dyn AppState>>,
}

// src/ui/apps/cleanup_app.rs
pub struct CleanupApp {
    pub base: AppBase,
    pub cleanup_branches: Vec<CleanupBranch>,
    pub initial_state: Option<Box<dyn AppState>>,
}
```

### 4. Unified App Enum for State Trait

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
            AppConfig::Default { .. } => {
                App::Merge(MergeApp::new(config, client))
            }
            AppConfig::Migration { .. } => {
                App::Migration(MigrationApp::new(config, client))
            }
            AppConfig::Cleanup { .. } => {
                App::Cleanup(CleanupApp::new(config, client))
            }
        }
    }

    /// Access shared base state
    pub fn base(&self) -> &AppBase {
        match self {
            App::Merge(app) => &app.base,
            App::Migration(app) => &app.base,
            App::Cleanup(app) => &app.base,
        }
    }

    pub fn base_mut(&mut self) -> &mut AppBase {
        match self {
            App::Merge(app) => &mut app.base,
            App::Migration(app) => &mut app.base,
            App::Cleanup(app) => &mut app.base,
        }
    }

    /// Type-safe access to mode-specific apps
    pub fn as_merge(&self) -> Option<&MergeApp> { ... }
    pub fn as_merge_mut(&mut self) -> Option<&mut MergeApp> { ... }
    pub fn as_migration(&self) -> Option<&MigrationApp> { ... }
    pub fn as_migration_mut(&mut self) -> Option<&mut MigrationApp> { ... }
    pub fn as_cleanup(&self) -> Option<&CleanupApp> { ... }
    pub fn as_cleanup_mut(&mut self) -> Option<&mut CleanupApp> { ... }
}
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
   - Include `WorktreeContext` as a field

3. **Create `src/ui/apps/` directory**
   - `src/ui/apps/mod.rs` - exports
   - `src/ui/apps/merge_app.rs` - MergeApp struct
   - `src/ui/apps/migration_app.rs` - MigrationApp struct
   - `src/ui/apps/cleanup_app.rs` - CleanupApp struct

### Phase 2: Refactor App Enum

4. **Update `src/ui/app.rs`**
   - Change `App` from struct to enum
   - Add `from_config()` factory method
   - Add `base()`, `base_mut()` accessors
   - Add type-safe mode accessors
   - Keep old field accessors as convenience methods that delegate to base

5. **Update `src/ui/mod.rs`**
   - Export new modules
   - Update re-exports

### Phase 3: Update State Modules

6. **Update Default/Merge states** (`src/ui/state/default/`)
   - Update field access: `app.cherry_pick_items` → `app.as_merge().unwrap().cherry_pick_items`
   - Or use pattern matching: `if let App::Merge(merge_app) = app { ... }`
   - Update worktree access: `app.base_repo_path` → `app.base().worktree.base_repo_path`

7. **Update Migration states** (`src/ui/state/migration/`)
   - Update field access to use `app.as_migration_mut()`
   - Update worktree tracking to use `app.base_mut().worktree`
   - Remove `app.migration_worktree_id` usage, use `app.base().worktree.worktree_id`

8. **Update Cleanup states** (`src/ui/state/cleanup/`)
   - Update field access to use `app.as_cleanup_mut()`

9. **Update Shared states** (`src/ui/state/shared/`)
   - Use `app.base()` for shared access

### Phase 4: Update Entry Points

10. **Update `src/ui/mod.rs` - `run_app_with_events()`**
    - Update to work with new App enum

11. **Update `src/bin/mergers.rs`**
    - Update App creation to use `App::from_config()`

### Phase 5: Cleanup and Tests

12. **Remove old code**
    - Remove mode-specific fields from old App struct
    - Remove `cleanup_migration_worktree()` from App (now in WorktreeContext)
    - Remove Drop impl from App (now in WorktreeContext)

13. **Update tests**
    - Update test helpers in `src/ui/testing.rs`
    - Update snapshot tests
    - Add new tests for WorktreeContext
    - Add tests for type-safe accessors

## File Changes Summary

### New Files
- `src/ui/worktree_context.rs`
- `src/ui/app_base.rs`
- `src/ui/apps/mod.rs`
- `src/ui/apps/merge_app.rs`
- `src/ui/apps/migration_app.rs`
- `src/ui/apps/cleanup_app.rs`

### Modified Files
- `src/ui/mod.rs` - exports and run_app updates
- `src/ui/app.rs` - struct → enum refactor
- `src/ui/state/default/*.rs` - all files need access pattern updates
- `src/ui/state/migration/*.rs` - all files need access pattern updates
- `src/ui/state/cleanup/*.rs` - all files need access pattern updates
- `src/ui/state/shared/*.rs` - use base() accessor
- `src/ui/testing.rs` - test helper updates
- `src/bin/mergers.rs` - App creation update

## Migration Pattern for State Files

### Before (current):
```rust
async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
    // Direct field access
    app.cherry_pick_items.push(item);
    app.migration_analysis = Some(analysis);
    app.base_repo_path = Some(path);
}
```

### After (new):
```rust
async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
    // For merge-specific state
    if let App::Merge(merge_app) = app {
        merge_app.cherry_pick_items.push(item);
    }

    // For migration-specific state
    if let App::Migration(migration_app) = app {
        migration_app.migration_analysis = Some(analysis);
    }

    // For shared worktree state
    app.base_mut().worktree.base_repo_path = Some(path);
}
```

## Benefits

1. **Type Safety**: Can't access `cherry_pick_items` in migration mode - it doesn't exist
2. **Clear Ownership**: WorktreeContext owns cleanup responsibility
3. **Reduced Bloat**: Each app type only has relevant fields
4. **Extensibility**: Easy to add new modes
5. **Better Testing**: Can test mode-specific logic in isolation

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Large refactor touching many files | Phase the implementation, keep tests passing |
| Pattern matching verbosity | Add convenience methods on App enum |
| Breaking existing tests | Update test helpers first |

## Questions Resolved

- **Separate app types vs enum**: Using enum wrapper for unified trait compatibility
- **base_repo_path location**: Moved to shared WorktreeContext
- **Refactor scope**: Moderate - full separation but with convenience accessors
