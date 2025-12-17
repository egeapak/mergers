# Plan: TypedAppState Migration

## Overview

Migrate all individual states from the legacy `AppState` trait to `TypedAppState`, providing compile-time type safety for mode-specific states.

## Current State

- States implement `AppState` trait with `&App` (enum)
- State transitions use `StateChange::Change(Box::new(NextState::new()))`
- Run loop uses `Box<dyn AppState>`

## Target State

- States implement `TypedAppState` with mode-specific app types (`&MergeApp`, etc.)
- State transitions use `TypedStateChange::Change(MergeState::Variant(state))`
- Run loop uses typed state enums directly
- Legacy `AppState` trait removed

---

## Phase A: Prerequisites

### A.1 Add Accessor Methods to AppBase

States use method syntax like `app.client()`, `app.pull_requests_mut()`. These exist on `App` enum but not on `AppBase`. Since mode-specific apps deref to `AppBase`, we need these methods on `AppBase`.

**Add to `src/ui/app_base.rs`:**
```rust
// Field accessors (currently only on App enum)
pub fn client(&self) -> &AzureDevOpsClient { &self.client }
pub fn pull_requests(&self) -> &Vec<PullRequestWithWorkItems> { &self.pull_requests }
pub fn pull_requests_mut(&mut self) -> &mut Vec<PullRequestWithWorkItems> { &mut self.pull_requests }
pub fn version(&self) -> Option<&str> { self.version.as_deref() }
pub fn set_version(&mut self, v: Option<String>) { self.version = v; }
pub fn error_message(&self) -> Option<&str> { self.error_message.as_deref() }
pub fn set_error_message(&mut self, msg: Option<String>) { self.error_message = msg; }
pub fn repo_path(&self) -> Option<&Path> { self.worktree.repo_path() }
pub fn set_repo_path(&mut self, path: Option<PathBuf>) { self.worktree.set_repo_path(path); }
```

### A.2 Add Mode-Specific Accessors

**Add to `src/ui/apps/merge_app.rs`:**
```rust
pub fn cherry_pick_items(&self) -> &Vec<CherryPickItem> { &self.cherry_pick_items }
pub fn cherry_pick_items_mut(&mut self) -> &mut Vec<CherryPickItem> { &mut self.cherry_pick_items }
pub fn current_cherry_pick_index(&self) -> usize { self.current_cherry_pick_index }
pub fn set_current_cherry_pick_index(&mut self, idx: usize) { self.current_cherry_pick_index = idx; }
```

**Add to `src/ui/apps/migration_app.rs`:**
```rust
pub fn migration_analysis(&self) -> Option<&MigrationAnalysis> { self.migration_analysis.as_ref() }
pub fn migration_analysis_mut(&mut self) -> Option<&mut MigrationAnalysis> { self.migration_analysis.as_mut() }
pub fn set_migration_analysis(&mut self, a: Option<MigrationAnalysis>) { self.migration_analysis = a; }
```

**Add to `src/ui/apps/cleanup_app.rs`:**
```rust
pub fn cleanup_branches(&self) -> &Vec<CleanupBranch> { &self.cleanup_branches }
pub fn cleanup_branches_mut(&mut self) -> &mut Vec<CleanupBranch> { &mut self.cleanup_branches }
```

---

## Phase B: Migrate Shared States

Shared states (ErrorState, SettingsConfirmationState) are used by all modes. They need to be generic over the app type and state enum.

### B.1 Update ErrorState

**File:** `src/ui/state/shared/error.rs`

Change from:
```rust
impl AppState for ErrorState {
    fn ui(&mut self, f: &mut Frame, app: &App) { ... }
    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange { ... }
}
```

To generic implementation that can be specialized per mode:
```rust
impl ErrorState {
    pub fn ui_impl(&mut self, f: &mut Frame, error_message: Option<&str>) { ... }

    pub fn process_key_impl<S>(&mut self, code: KeyCode) -> TypedStateChange<S> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => TypedStateChange::Exit,
            _ => TypedStateChange::Keep,
        }
    }
}
```

Then in each state enum file, implement TypedAppState by delegating to these helpers.

### B.2 Update SettingsConfirmationState

**File:** `src/ui/state/shared/settings_confirmation.rs`

Similar pattern - extract UI/logic into impl methods, then have mode-specific TypedAppState implementations that wrap and delegate.

---

## Phase C: Migrate Merge Mode States (9 states)

### Migration Pattern for Each State

1. Change imports: `App` → `MergeApp`, `StateChange` → `TypedStateChange`, add `MergeState`
2. Change trait impl: `impl AppState` → `impl TypedAppState`
3. Change associated types: `type App = MergeApp; type StateEnum = MergeState;`
4. Change method signatures: `&App` → `&MergeApp`, `&mut App` → `&mut MergeApp`
5. Change return types: `StateChange` → `TypedStateChange<MergeState>`
6. Change state transitions:
   - `StateChange::Keep` → `TypedStateChange::Keep`
   - `StateChange::Exit` → `TypedStateChange::Exit`
   - `StateChange::Change(Box::new(NextState::new()))` → `TypedStateChange::Change(MergeState::NextState(NextState::new()))`

### C.1 DataLoadingState
**File:** `src/ui/state/default/data_loading.rs`
- Transitions to: PullRequestSelectionState, ErrorState

### C.2 PullRequestSelectionState
**File:** `src/ui/state/default/pr_selection.rs`
- Transitions to: VersionInputState, ErrorState

### C.3 VersionInputState
**File:** `src/ui/state/default/version_input.rs`
- Transitions to: SetupRepoState, PullRequestSelectionState

### C.4 SetupRepoState
**File:** `src/ui/state/default/setup_repo.rs`
- Transitions to: CherryPickState, ErrorState

### C.5 CherryPickState
**File:** `src/ui/state/default/cherry_pick.rs`
- Transitions to: ConflictResolutionState, CompletionState, ErrorState

### C.6 ConflictResolutionState
**File:** `src/ui/state/default/conflict_resolution.rs`
- Transitions to: CherryPickContinueState

### C.7 CherryPickContinueState
**File:** `src/ui/state/default/cherry_pick_continue.rs`
- Transitions to: CherryPickState, CompletionState, ErrorState

### C.8 CompletionState
**File:** `src/ui/state/default/completion.rs`
- Transitions to: PostCompletionState

### C.9 PostCompletionState
**File:** `src/ui/state/default/post_completion.rs`
- Transitions to: Exit

---

## Phase D: Migrate Migration Mode States (4 states)

### D.1 MigrationDataLoadingState
**File:** `src/ui/state/migration/data_loading.rs`
- Use `type App = MigrationApp; type StateEnum = MigrationModeState;`

### D.2 MigrationResultsState
**File:** `src/ui/state/migration/results.rs`

### D.3 MigrationVersionInputState
**File:** `src/ui/state/migration/version_input.rs`

### D.4 MigrationTaggingState
**File:** `src/ui/state/migration/tagging.rs`

---

## Phase E: Migrate Cleanup Mode States (4 states)

### E.1 CleanupDataLoadingState
**File:** `src/ui/state/cleanup/data_loading.rs`
- Use `type App = CleanupApp; type StateEnum = CleanupModeState;`

### E.2 CleanupBranchSelectionState
**File:** `src/ui/state/cleanup/branch_selection.rs`

### E.3 CleanupExecutionState
**File:** `src/ui/state/cleanup/cleanup_execution.rs`

### E.4 CleanupResultsState
**File:** `src/ui/state/cleanup/results.rs`

---

## Phase F: Update State Enum TypedAppState Implementations

### F.1 Update MergeState
**File:** `src/ui/state/default/state_enum.rs`

Replace placeholder with actual dispatch:
```rust
#[async_trait]
impl TypedAppState for MergeState {
    type App = MergeApp;
    type StateEnum = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        match self {
            MergeState::DataLoading(state) => state.ui(f, app),
            MergeState::PullRequestSelection(state) => state.ui(f, app),
            // ... all variants
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp)
        -> TypedStateChange<MergeState>
    {
        match self {
            MergeState::DataLoading(state) => state.process_key(code, app).await,
            // ... all variants
        }
    }
}
```

### F.2 Update MigrationModeState
**File:** `src/ui/state/migration/state_enum.rs`

### F.3 Update CleanupModeState
**File:** `src/ui/state/cleanup/state_enum.rs`

---

## Phase G: Update Run Loop

### G.1 Update src/ui/mod.rs

Create typed run functions:

```rust
pub async fn run_merge_mode<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut MergeApp,
    event_source: &dyn EventSource,
) -> anyhow::Result<()> {
    let mut state = MergeState::initial();
    loop {
        terminal.draw(|f| state.ui(f, app))?;

        if let Some(event) = event_source.next_event().await? {
            let change = match event {
                Event::Key(key) => state.process_key(key.code, app).await,
                Event::Mouse(mouse) => state.process_mouse(mouse, app).await,
                _ => TypedStateChange::Keep,
            };

            match change {
                TypedStateChange::Keep => {}
                TypedStateChange::Change(new_state) => state = new_state,
                TypedStateChange::Exit => break,
            }
        }
    }
    Ok(())
}

// Similar for run_migration_mode and run_cleanup_mode
```

### G.2 Update main run function

```rust
pub async fn run_app<B: Backend>(...) -> anyhow::Result<()> {
    match app {
        App::Merge(ref mut merge_app) => run_merge_mode(terminal, merge_app, event_source).await,
        App::Migration(ref mut migration_app) => run_migration_mode(terminal, migration_app, event_source).await,
        App::Cleanup(ref mut cleanup_app) => run_cleanup_mode(terminal, cleanup_app, event_source).await,
    }
}
```

---

## Phase H: Update Tests and Helpers

### H.1 Update TuiTestHarness
**File:** `src/ui/testing.rs`

Update to work with typed state enums.

### H.2 Update State Tests

Each state's test module needs updates:
- Use mode-specific app types in tests
- Update state transition assertions

---

## Phase I: Remove Legacy Code

### I.1 Remove from src/ui/state/mod.rs:
- `AppState` trait
- `StateChange` enum
- `create_initial_state` function (replace with mode-specific)

### I.2 Remove AppState implementations from state_enum.rs files

### I.3 Clean up unused imports across all files

---

## Execution Order

1. **Phase A** - Add accessor methods (foundation)
2. **Phase B** - Migrate shared states (needed by all modes)
3. **Phase C** - Migrate merge mode states (largest set)
4. **Phase F.1** - Update MergeState enum dispatch
5. **Phase G** - Update run loop (can test merge mode end-to-end)
6. **Phase D** - Migrate migration mode states
7. **Phase F.2** - Update MigrationModeState enum dispatch
8. **Phase E** - Migrate cleanup mode states
9. **Phase F.3** - Update CleanupModeState enum dispatch
10. **Phase H** - Update tests
11. **Phase I** - Remove legacy code

## Testing Strategy

- Run `cargo test` after each state migration
- Run `cargo clippy` after each phase
- Keep both AppState and TypedAppState during migration
- Only remove AppState after all states migrated and tests pass

## Risk Mitigation

- Commit after each phase
- If tests fail, can revert to last working commit
- Keep legacy implementations until fully migrated
