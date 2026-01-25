# Channel-Based Wizard Step Execution - Progress Tracker

## Status: Not Started

**Last Updated**: 2025-01-25
**Current Phase**: Phase 1 - Message Types
**Blocked By**: None

---

## Phases Overview

| Phase | Description | Status | Completion |
|-------|-------------|--------|------------|
| 1 | Message Types and Context Extraction | ⬜ Not Started | 0% |
| 2 | State Restructuring | ⬜ Not Started | 0% |
| 3 | Background Task Implementation | ⬜ Not Started | 0% |
| 4 | UI Integration | ⬜ Not Started | 0% |
| 5 | Error Handling | ⬜ Not Started | 0% |
| 6 | Testing | ⬜ Not Started | 0% |

---

## Phase 1: Message Types and Context Extraction

### Tasks

- [ ] Create `ProgressMessage` enum with variants:
  - [ ] `StepStarted(WizardStep)`
  - [ ] `StepCompleted { step, result }`
  - [ ] `AllComplete`
  - [ ] `Error { step, error }`
- [ ] Create `StepResult` enum with variants:
  - [ ] `FetchDetails { ssh_url }`
  - [ ] `CloneComplete { path, temp_dir }`
  - [ ] `WorktreeComplete { path, base_path }`
  - [ ] `BranchCreated { branch_name }`
  - [ ] `CherryPicksPrepared { items }`
  - [ ] `None`
- [ ] Create `SetupError` enum:
  - [ ] `Setup(git::RepositorySetupError)`
  - [ ] `General(String)`
- [ ] Create `SelectedPrData` struct
- [ ] Create `SetupContext` struct
- [ ] Implement `SetupContext::from_app()`
- [ ] Add unit tests for context extraction

### Files Modified

- `src/ui/state/default/setup_repo.rs`

### Notes

- All types should derive `Debug`, `Clone` where applicable
- `TempDir` is `Send` so can be sent through channel
- Consider if `StepResult` needs to be `Send + 'static`

---

## Phase 2: State Restructuring

### Tasks

- [ ] Refactor `SetupState` enum:
  - [ ] `Idle` - waiting to start
  - [ ] `Running { progress, progress_rx, task_handle, pending_results }`
  - [ ] `Complete`
  - [ ] `Error { error, message, progress }`
- [ ] Update `SetupRepoState` struct
- [ ] Remove old `StepData` struct (data now in `StepResult`)
- [ ] Remove `started` field (replaced by `Idle` state)
- [ ] Add helper methods:
  - [ ] `start_task()`
  - [ ] `poll_progress()`
  - [ ] `apply_pending_results()`
- [ ] Ensure `SetupRepoState` remains `!Send` if needed (due to `Receiver`)

### Files Modified

- `src/ui/state/default/setup_repo.rs`

### Notes

- `JoinHandle<()>` is `Send` so state can still work with async trait
- Consider storing SSH URL in `Running` state for clone step

---

## Phase 3: Background Task Implementation

### Tasks

- [ ] Create `run_setup_task()` async function
- [ ] Implement step 1: FetchDetails
  - [ ] Send `StepStarted`
  - [ ] Call `client.fetch_repo_details().await`
  - [ ] Send `StepCompleted` with SSH URL or `Error`
- [ ] Implement step 2: CheckPrerequisites
  - [ ] Validate SSH URL (clone mode) or local repo path (worktree mode)
  - [ ] Send appropriate messages
- [ ] Implement step 3: FetchTargetBranch (worktree mode only)
  - [ ] Run `git fetch origin {target_branch}`
  - [ ] Handle errors
- [ ] Implement step 4: CloneOrWorktree
  - [ ] Clone mode: `git::shallow_clone_repo()`
  - [ ] Worktree mode: `git::create_worktree()`
  - [ ] Send result with paths
- [ ] Implement step 5: CreateBranch
  - [ ] `git::create_branch()`
  - [ ] Handle "branch exists" error specially
- [ ] Implement step 6: PrepareCherryPicks
  - [ ] Build `CherryPickItem` list from context
  - [ ] Validate non-empty
- [ ] Implement step 7: InitializeState
  - [ ] Note: Actual state file creation in UI
  - [ ] Just send completion message
- [ ] Send `AllComplete` at end

### Files Modified

- `src/ui/state/default/setup_repo.rs`

### Dependencies

- Phase 1 complete (message types)
- Phase 2 complete (state structure)

### Notes

- Use macro for repetitive send pattern
- Handle channel disconnect (receiver dropped)
- Clone `AzureDevOpsClient` before spawning (it should be `Clone`)

---

## Phase 4: UI Integration

### Tasks

- [ ] Update `process_key()` for `Idle` state:
  - [ ] Extract context from app
  - [ ] Clone API client
  - [ ] Create channel
  - [ ] Spawn background task
  - [ ] Transition to `Running`
- [ ] Update `process_key()` for `Running` state:
  - [ ] Poll channel with `try_recv()`
  - [ ] Update progress on `StepStarted`
  - [ ] Queue results on `StepCompleted`
  - [ ] Transition on `AllComplete`
  - [ ] Set error state on `Error`
- [ ] Implement `apply_result()` method:
  - [ ] Handle each `StepResult` variant
  - [ ] Update `MergeApp` state
- [ ] Implement `create_state_file()` method
- [ ] Update `process_key()` for `Complete` state
- [ ] Update `ui()` method for new state structure:
  - [ ] `Idle` -> show initializing UI
  - [ ] `Running` -> show progress
  - [ ] `Complete` -> shouldn't render (immediate transition)
  - [ ] `Error` -> show error UI

### Files Modified

- `src/ui/state/default/setup_repo.rs`

### Dependencies

- Phase 3 complete (background task)

### Notes

- Remember to apply pending results after polling
- State file must be created in UI (needs `&mut MergeApp`)

---

## Phase 5: Error Handling

### Tasks

- [ ] Update `set_error()` method for new error types
- [ ] Implement error state in `process_key()`:
  - [ ] 'r' for retry (abort task, reset to Idle)
  - [ ] 'f' for force resolve
  - [ ] Esc to exit
- [ ] Port `force_resolve_error()` logic:
  - [ ] Handle `BranchExists`
  - [ ] Handle `WorktreeExists`
  - [ ] Handle `Other`
- [ ] Implement task abort on retry:
  - [ ] Call `task_handle.abort()`
  - [ ] Drop receiver
- [ ] Handle task panic:
  - [ ] Check `JoinHandle` for panic
  - [ ] Show appropriate error

### Files Modified

- `src/ui/state/default/setup_repo.rs`

### Dependencies

- Phase 4 complete (UI integration)

### Notes

- `JoinHandle::abort()` is safe to call
- Receiver drop will cause sender to fail

---

## Phase 6: Testing

### Tasks

- [ ] Update snapshot tests:
  - [ ] `test_setup_repo_initializing` (now Idle state)
  - [ ] `test_setup_repo_fetch_details`
  - [ ] `test_setup_repo_check_prerequisites`
  - [ ] `test_setup_repo_cloning`
  - [ ] `test_setup_repo_fetch_target_branch`
  - [ ] `test_setup_repo_creating_worktree`
  - [ ] `test_setup_repo_creating_branch`
  - [ ] `test_setup_repo_preparing_cherry_picks`
  - [ ] `test_setup_repo_initializing_state`
  - [ ] `test_setup_repo_clone_mode_all_complete`
  - [ ] `test_setup_repo_error_with_progress`
  - [ ] `test_setup_repo_branch_exists_error`
  - [ ] `test_setup_repo_worktree_exists_error`
  - [ ] `test_setup_repo_other_error`
- [ ] Add unit tests:
  - [ ] `test_setup_context_from_app`
  - [ ] `test_apply_result_clone_complete`
  - [ ] `test_apply_result_worktree_complete`
  - [ ] `test_apply_result_cherry_picks`
- [ ] Add async tests:
  - [ ] `test_progress_message_flow`
  - [ ] `test_error_message_handling`
  - [ ] `test_task_cancellation`
- [ ] Run full test suite
- [ ] Update any broken tests

### Files Modified

- `src/ui/state/default/setup_repo.rs`
- Snapshot files in `src/ui/snapshots/state/default/setup_repo/`

### Dependencies

- All previous phases complete

### Notes

- May need mock API client for async tests
- Consider using `tokio::sync::mpsc::channel` in tests

---

## Verification Checklist

See `docs/design/wizard-channel-verification.md` for detailed verification steps.

---

## Blockers & Risks

| Issue | Impact | Mitigation |
|-------|--------|------------|
| `AzureDevOpsClient` not `Clone` | High | Check if it's `Clone`, or wrap in `Arc` |
| `TempDir` Send issues | High | Verify `TempDir` is `Send` |
| Task lifetime complexity | Medium | Careful handling of abort/drop |
| Test complexity | Medium | Mock API client, use `tokio-test` |

---

## Notes

- Keep current tick-based implementation in git for reference
- Channel buffer size of 32 should be sufficient
- Consider adding timeout for stuck steps (future enhancement)

---

## Session Log

### Session 1 (2025-01-25)
- Created implementation plan document
- Created progress tracker document
- Created verification document
- Pushed initial tick-based sub-step implementation
- Discussed channel-based alternative with user
