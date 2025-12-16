# Progress: Mode-Specific App Types Refactoring

Track implementation progress for the mode separation refactoring.

**Last Updated:** 2025-12-16
**Status:** Phase 1-3 Complete

## Overview

| Phase | Status | Progress |
|-------|--------|----------|
| Phase 1: Core Infrastructure | ✅ Complete | 3/3 |
| Phase 2: Mode-Specific App Types | ✅ Complete | 4/4 |
| Phase 3: State Infrastructure | ✅ Complete | 5/5 |
| Phase 4: Mode-Specific States | ⬜ Not Started | 0/3 |
| Phase 5: Run Loop & Entry Points | ⬜ Not Started | 0/2 |
| Phase 6: Cleanup & Tests | ⬜ Not Started | 0/2 |

---

## Phase 1: Core Infrastructure

### 1.1 Create `src/ui/worktree_context.rs` ✅
- [x] Define `WorktreeContext` struct with fields:
  - `repo_path: Option<PathBuf>`
  - `base_repo_path: Option<PathBuf>`
  - `_temp_dir: Option<TempDir>`
  - `worktree_id: Option<String>`
- [x] Implement `cleanup()` method
- [x] Implement `Drop` trait
- [x] Add unit tests (7 tests)

**Files:** `src/ui/worktree_context.rs` (new)

### 1.2 Create `src/ui/app_base.rs` ✅
- [x] Define `AppBase` struct with shared fields:
  - `config: Arc<AppConfig>`
  - `pull_requests: Vec<PullRequestWithWorkItems>`
  - `client: AzureDevOpsClient`
  - `version: Option<String>`
  - `worktree: WorktreeContext`
  - `error_message: Option<String>`
- [x] Move configuration getter methods from App
- [x] Move shared helper methods (open_pr_in_browser, etc.)
- [x] Add unit tests (4 tests)

**Files:** `src/ui/app_base.rs` (new)

### 1.3 Create `src/ui/app_mode.rs` ✅
- [x] Define `AppMode` trait with methods:
  - `fn base(&self) -> &AppBase`
  - `fn base_mut(&mut self) -> &mut AppBase`

**Files:** `src/ui/app_mode.rs` (new)

---

## Phase 2: Mode-Specific App Types

### 2.1 Create `src/ui/apps/merge_app.rs`
- [ ] Define `MergeApp` struct with fields:
  - `base: AppBase`
  - `cherry_pick_items: Vec<CherryPickItem>`
  - `current_cherry_pick_index: usize`
- [ ] Implement `Deref` and `DerefMut` to `AppBase`
- [ ] Implement `AppMode` trait
- [ ] Add merge-specific methods (work_item_state, etc.)

**Files:** `src/ui/apps/merge_app.rs` (new)

### 2.2 Create `src/ui/apps/migration_app.rs`
- [ ] Define `MigrationApp` struct with fields:
  - `base: AppBase`
  - `migration_analysis: Option<MigrationAnalysis>`
  - `manual_overrides: HashMap<i32, bool>` (if applicable)
- [ ] Implement `Deref` and `DerefMut` to `AppBase`
- [ ] Implement `AppMode` trait
- [ ] Move manual override methods from App

**Files:** `src/ui/apps/migration_app.rs` (new)

### 2.3 Create `src/ui/apps/cleanup_app.rs`
- [ ] Define `CleanupApp` struct with fields:
  - `base: AppBase`
  - `cleanup_branches: Vec<CleanupBranch>`
- [ ] Implement `Deref` and `DerefMut` to `AppBase`
- [ ] Implement `AppMode` trait
- [ ] Add cleanup-specific methods

**Files:** `src/ui/apps/cleanup_app.rs` (new)

### 2.4 Create `src/ui/apps/mod.rs`
- [ ] Export `MergeApp`, `MigrationApp`, `CleanupApp`

**Files:** `src/ui/apps/mod.rs` (new)

### 2.5 Update `src/ui/app.rs`
- [ ] Change `App` from struct to enum:
  - `Merge(MergeApp)`
  - `Migration(MigrationApp)`
  - `Cleanup(CleanupApp)`
- [ ] Add `from_config()` factory method
- [ ] Implement `Deref`/`DerefMut` to `AppBase`

**Files:** `src/ui/app.rs` (modify)

### 2.6 Update `src/ui/mod.rs`
- [ ] Add exports for new modules

**Files:** `src/ui/mod.rs` (modify)

---

## Phase 3: State Infrastructure ✅

### 3.1 Create `src/ui/state/typed.rs` ✅
- [x] Define `TypedAppState` trait with associated types:
  - `type App: AppMode + Send + Sync`
  - `type StateEnum: Send`
- [x] Define methods: `ui()`, `process_key()`, `process_mouse()`, `name()`
- [x] Define `TypedStateChange<S>` generic enum (Keep, Change, Exit)
- [x] Add helper methods: `is_keep()`, `is_change()`, `is_exit()`, `map()`
- [x] Add unit tests (5 tests)

**Files:** `src/ui/state/typed.rs` (new)

### 3.2 Create MergeState enum ✅
- [x] Define `MergeState` enum in `src/ui/state/default/state_enum.rs`
- [x] Variants: SettingsConfirmation, DataLoading, PullRequestSelection,
      VersionInput, SetupRepo, CherryPick, ConflictResolution,
      CherryPickContinue, Completion, PostCompletion, Error
- [x] Implement `initial()` and `initial_with_confirmation()` methods
- [x] Implement `name()` method for debugging
- [x] Implement manual `Debug` trait
- [x] Placeholder `TypedAppState` implementation
- [x] Box larger variants to satisfy clippy
- [x] Add unit tests (4 tests)

**Files:** `src/ui/state/default/state_enum.rs` (new)

### 3.3 Create MigrationModeState enum ✅
- [x] Define `MigrationModeState` enum in `src/ui/state/migration/state_enum.rs`
- [x] Variants: SettingsConfirmation, DataLoading, Results, VersionInput,
      Tagging, Error
- [x] Implement `initial()` and `initial_with_confirmation()` methods
- [x] Implement `name()` method for debugging
- [x] Implement manual `Debug` trait
- [x] Placeholder `TypedAppState` implementation
- [x] Box larger variants to satisfy clippy
- [x] Add unit tests (4 tests)

**Files:** `src/ui/state/migration/state_enum.rs` (new)

### 3.4 Create CleanupModeState enum ✅
- [x] Define `CleanupModeState` enum in `src/ui/state/cleanup/state_enum.rs`
- [x] Variants: SettingsConfirmation, DataLoading, BranchSelection,
      Execution, Results, Error
- [x] Implement `initial()` and `initial_with_confirmation()` methods
- [x] Implement `name()` method for debugging
- [x] Implement manual `Debug` trait
- [x] Placeholder `TypedAppState` implementation
- [x] Box larger variants to satisfy clippy
- [x] Add unit tests (4 tests)

**Files:** `src/ui/state/cleanup/state_enum.rs` (new)

### 3.5 Create Typed Shared States ✅
- [x] Create `TypedErrorState<A, S>` in `src/ui/state/shared/typed_error.rs`
- [x] Create `TypedSettingsConfirmationState<A, S>` in
      `src/ui/state/shared/typed_settings_confirmation.rs`
- [x] Both use `PhantomData` for type parameters
- [x] Both implement manual `Debug` trait
- [x] Update module exports
- [x] Add unit tests (6 tests total)

**Files:**
- `src/ui/state/shared/typed_error.rs` (new)
- `src/ui/state/shared/typed_settings_confirmation.rs` (new)
- `src/ui/state/shared/mod.rs` (modify)
- `src/ui/state/mod.rs` (modify)

---

## Phase 4: Update Mode-Specific States

### 4.1 Merge/Default States

| File | Status | Notes |
|------|--------|-------|
| `state/default/data_loading.rs` | ⬜ | `type App = MergeApp` |
| `state/default/setup_repo.rs` | ⬜ | `type App = MergeApp` |
| `state/default/version_input.rs` | ⬜ | `type App = MergeApp` |
| `state/default/pr_selection.rs` | ⬜ | `type App = MergeApp` |
| `state/default/cherry_pick.rs` | ⬜ | `type App = MergeApp` |
| `state/default/cherry_pick_continue.rs` | ⬜ | `type App = MergeApp` |
| `state/default/conflict_resolution.rs` | ⬜ | `type App = MergeApp` |
| `state/default/completion.rs` | ⬜ | `type App = MergeApp` |
| `state/default/post_completion.rs` | ⬜ | `type App = MergeApp` |

### 4.2 Migration States

| File | Status | Notes |
|------|--------|-------|
| `state/migration/data_loading.rs` | ⬜ | `type App = MigrationApp` |
| `state/migration/version_input.rs` | ⬜ | `type App = MigrationApp` |
| `state/migration/results.rs` | ⬜ | `type App = MigrationApp` |
| `state/migration/tagging.rs` | ⬜ | `type App = MigrationApp` |

### 4.3 Cleanup States

| File | Status | Notes |
|------|--------|-------|
| `state/cleanup/data_loading.rs` | ⬜ | `type App = CleanupApp` |
| `state/cleanup/branch_selection.rs` | ⬜ | `type App = CleanupApp` |
| `state/cleanup/cleanup_execution.rs` | ⬜ | `type App = CleanupApp` |
| `state/cleanup/results.rs` | ⬜ | `type App = CleanupApp` |

---

## Phase 5: Update Run Loop & Entry Points

### 5.1 Update `src/ui/mod.rs` - Run Functions
- [ ] Update `run_app_with_events()` to match on App enum
- [ ] Create `run_merge_mode()` function
- [ ] Create `run_migration_mode()` function
- [ ] Create `run_cleanup_mode()` function

**Files:** `src/ui/mod.rs` (modify)

### 5.2 Update `src/bin/mergers.rs`
- [ ] Update App creation to use `App::from_config()`

**Files:** `src/bin/mergers.rs` (modify)

---

## Phase 6: Cleanup & Tests

### 6.1 Cleanup Old Code
- [ ] Remove mode-specific fields from old App struct (now enum)
- [ ] Remove `cleanup_migration_worktree()` (now in WorktreeContext)
- [ ] Remove Drop impl from App (now in WorktreeContext)
- [ ] Remove `initial_state` field (now managed by state enums)

### 6.2 Update Tests
- [ ] Update test helpers in `src/ui/testing.rs`
- [ ] Add tests for `WorktreeContext`
- [ ] Add tests for mode-specific apps
- [ ] Update snapshot tests to use mode-specific apps
- [ ] Update existing state tests for new structure
- [ ] Run full test suite and fix any issues

**Files:**
- `src/ui/testing.rs` (modify)
- Test files in state directories (modify)

---

## Verification Checklist

After completion, verify:
- [ ] All tests pass (`cargo nextest run`)
- [ ] No clippy warnings (`cargo clippy --all-targets --all-features -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --check`)
- [ ] Migration worktree cleanup works on exit
- [ ] Each mode starts correctly
- [ ] State transitions work within each mode
- [ ] Shared states (settings, error) work in all modes

---

## Notes

- Keep tests passing after each phase
- Can be done incrementally - phases 1-2 can be done without breaking existing code
- Phase 3+ requires updating all state files simultaneously
- Consider feature branch for phases 3+
