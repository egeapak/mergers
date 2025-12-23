# Non-Interactive Merge Mode - Implementation Progress

## Overview

This document tracks the implementation progress of the non-interactive merge mode feature.

**Branch:** `claude/add-noninteractive-merge-mode-nGPeX`
**Started:** 2024-12-22
**Last Updated:** 2024-12-23

---

## Progress Summary

| Phase | Status | Progress |
|-------|--------|----------|
| Phase 1: Foundation | Complete | 100% |
| Phase 2: Core Operations | Not Started | 0% |
| Phase 3: Output System | Not Started | 0% |
| Phase 4: Non-Interactive Runner | Not Started | 0% |
| Phase 5: Entry Point Integration | Not Started | 0% |
| Phase 6: Interactive Mode Integration | Not Started | 0% |
| Phase 7: Testing & Documentation | Not Started | 0% |

**Overall Progress:** ~14%

---

## Phase 1: Foundation (Core Infrastructure)

**Status:** Complete ✅
**Estimated Effort:** Medium

### Tasks

| Task | Status | Notes |
|------|--------|-------|
| Create `src/core/mod.rs` module structure | ✅ Complete | |
| Define `ExitCode` enum | ✅ Complete | |
| Create `src/core/state/mod.rs` | ✅ Complete | |
| Implement `MergeStateFile` struct | ✅ Complete | |
| Implement `StateCherryPickItem` struct | ✅ Complete | |
| Implement `MergePhase` enum | ✅ Complete | |
| Implement `MergeStatus` enum | ✅ Complete | |
| Implement `StateItemStatus` enum | ✅ Complete | |
| Implement per-repo path hashing | ✅ Complete | SHA-256, first 16 chars |
| Implement `state_dir()` with XDG + env override | ✅ Complete | |
| Implement `path_for_repo()` | ✅ Complete | |
| Implement `lock_path_for_repo()` | ✅ Complete | |
| Implement `LockGuard` struct | ✅ Complete | |
| Implement `acquire_lock()` | ✅ Complete | |
| Implement stale lock detection | ✅ Complete | Check if PID still running |
| Implement `save()` for state file | ✅ Complete | Atomic write |
| Implement `load()` for state file | ✅ Complete | |
| Add `run_hooks` field to MergeStateFile | ✅ Complete | Captures setting at start |
| Add `sha2` dependency to Cargo.toml | ✅ Complete | |
| Add `dirs` dependency to Cargo.toml | ✅ Complete | Already present |
| Add `libc` dependency to Cargo.toml | ✅ Complete | For Unix process check |
| Add `MergeSubcommand` enum to models.rs | ✅ Complete | |
| Add `MergeRunArgs` struct | ✅ Complete | Include --run-hooks flag |
| Add `MergeContinueArgs` struct | ✅ Complete | |
| Add `MergeAbortArgs` struct | ✅ Complete | |
| Add `MergeStatusArgs` struct | ✅ Complete | |
| Add `MergeCompleteArgs` struct | ✅ Complete | |
| Add `OutputFormat` enum | ✅ Complete | text, json, ndjson |
| Write unit tests for state file | ✅ Complete | |
| Write unit tests for path hashing | ✅ Complete | |
| Write unit tests for locking | ✅ Complete | |
| Write unit tests for run_hooks serialization | ✅ Complete | |

### Files Created/Modified

- [x] `src/core/mod.rs`
- [x] `src/core/state/mod.rs`
- [x] `src/core/state/file.rs`
- [x] `src/models.rs`
- [x] `Cargo.toml`
- [x] `src/lib.rs` (added core module export)

### Blockers

None

### Notes

- Use `libc::kill(pid, 0)` on Unix for process existence check
- Use Windows API on Windows for process existence check
- State file should use atomic write (write to temp, then rename)
- `run_hooks` must be captured at start and used consistently on resume

---

## Phase 2: Core Operations (Extract from UI)

**Status:** Not Started
**Estimated Effort:** Large

### Tasks

| Task | Status | Notes |
|------|--------|-------|
| Create `src/core/operations/mod.rs` | ⬜ Not Started | |
| Create `src/core/operations/data_loading.rs` | ⬜ Not Started | |
| Extract `fetch_pull_requests()` | ⬜ Not Started | From DataLoadingState |
| Extract `fetch_work_items_parallel()` | ⬜ Not Started | From DataLoadingState |
| Create `src/core/operations/pr_selection.rs` | ⬜ Not Started | |
| Implement `filter_prs_by_work_item_states()` | ⬜ Not Started | |
| Implement `select_prs_by_work_item_states()` | ⬜ Not Started | |
| Create `src/core/operations/repository_setup.rs` | ⬜ Not Started | |
| Extract repository setup logic | ⬜ Not Started | From SetupRepoState |
| Pass `run_hooks` to setup_repository | ⬜ Not Started | Use value from MergeConfig |
| Create `src/core/operations/cherry_pick.rs` | ⬜ Not Started | |
| Extract `process_next_commit()` logic | ⬜ Not Started | From CherryPickState |
| Extract `continue_cherry_pick()` logic | ⬜ Not Started | From CherryPickContinueState |
| Create abort helper (reuse AbortingState) | ⬜ Not Started | |
| Create `src/core/operations/post_merge.rs` | ⬜ Not Started | |
| Extract `tag_pr()` | ⬜ Not Started | From PostCompletionState |
| Extract `update_work_item()` | ⬜ Not Started | From PostCompletionState |
| Create `src/core/state/conversion.rs` | ⬜ Not Started | |
| Implement CherryPickItem ↔ StateCherryPickItem | ⬜ Not Started | |
| Implement CherryPickStatus ↔ StateItemStatus | ⬜ Not Started | |
| Write unit tests for PR filtering | ⬜ Not Started | |
| Write unit tests for state conversions | ⬜ Not Started | |

### Files Created/Modified

- [ ] `src/core/operations/mod.rs`
- [ ] `src/core/operations/data_loading.rs`
- [ ] `src/core/operations/pr_selection.rs`
- [ ] `src/core/operations/repository_setup.rs`
- [ ] `src/core/operations/cherry_pick.rs`
- [ ] `src/core/operations/post_merge.rs`
- [ ] `src/core/state/conversion.rs`

### Blockers

- Depends on Phase 1 completion

### Notes

- PR selection must check: ALL work items match AND at least 1 work item exists
- Case-insensitive state matching
- Preserve original PR order after filtering
- Use `MergeConfig` type directly for type-safe config access
- Repository setup uses `run_hooks` from config (not `AppConfig` enum)

---

## Phase 3: Output System

**Status:** Not Started
**Estimated Effort:** Small

### Tasks

| Task | Status | Notes |
|------|--------|-------|
| Create `src/core/output/mod.rs` | ⬜ Not Started | |
| Create `src/core/output/events.rs` | ⬜ Not Started | |
| Define `ProgressEvent` enum | ⬜ Not Started | All event types |
| Implement serde serialization for events | ⬜ Not Started | |
| Create `src/core/output/format.rs` | ⬜ Not Started | |
| Implement `TextFormatter` | ⬜ Not Started | Human-readable |
| Implement `JsonFormatter` | ⬜ Not Started | Summary at end |
| Implement `NdjsonFormatter` | ⬜ Not Started | Streaming |
| Define `ConflictOutput` struct | ⬜ Not Started | |
| Define `StatusOutput` struct | ⬜ Not Started | |
| Define `SummaryOutput` struct | ⬜ Not Started | |
| Write unit tests for event serialization | ⬜ Not Started | |
| Write unit tests for formatters | ⬜ Not Started | |

### Files Created/Modified

- [ ] `src/core/output/mod.rs`
- [ ] `src/core/output/events.rs`
- [ ] `src/core/output/format.rs`

### Blockers

None

### Notes

- NDJSON = one JSON object per line
- Text format should use colors where appropriate (check if terminal)
- Quiet mode suppresses progress, shows only errors and final result

---

## Phase 4: Non-Interactive Runner

**Status:** Not Started
**Estimated Effort:** Large

### Tasks

| Task | Status | Notes |
|------|--------|-------|
| Create `src/core/runner/mod.rs` | ⬜ Not Started | |
| Create `src/core/runner/traits.rs` | ⬜ Not Started | |
| Define `MergeRunner` trait | ⬜ Not Started | |
| Create `src/core/runner/merge_engine.rs` | ⬜ Not Started | |
| Implement core orchestration logic | ⬜ Not Started | |
| Create `src/core/runner/non_interactive.rs` | ⬜ Not Started | |
| Implement `NonInteractiveRunner` struct | ⬜ Not Started | |
| Implement `run()` method | ⬜ Not Started | Start new merge |
| Implement `continue_merge()` method | ⬜ Not Started | Resume after conflict |
| Implement `abort()` method | ⬜ Not Started | Cleanup and cancel |
| Implement `status()` method | ⬜ Not Started | Show current state |
| Implement `complete()` method | ⬜ Not Started | Tag PRs, update WIs |
| Handle all exit codes | ⬜ Not Started | |
| Write unit tests for runner | ⬜ Not Started | |
| Write integration tests | ⬜ Not Started | |

### Files Created/Modified

- [ ] `src/core/runner/mod.rs`
- [ ] `src/core/runner/traits.rs`
- [ ] `src/core/runner/merge_engine.rs`
- [ ] `src/core/runner/non_interactive.rs`

### Blockers

- Depends on Phase 1, 2, 3 completion

### Notes

- `run()`: Load data → Filter PRs → Setup repo → Cherry-pick → Save state
- `continue_merge()`: Load state → Check conflicts resolved → Continue cherry-pick
- `abort()`: Load state → Cleanup (reuse AbortingState logic) → Update state
- `complete()`: Load state → Tag PRs → Update WIs → Mark completed
- `status()`: Load state → Format output

---

## Phase 5: Entry Point Integration

**Status:** Not Started
**Estimated Effort:** Medium

### Tasks

| Task | Status | Notes |
|------|--------|-------|
| Add subcommand parsing to mergers.rs | ⬜ Not Started | |
| Handle `merge` (no subcommand) → TUI | ⬜ Not Started | |
| Handle `merge run` routing | ⬜ Not Started | Interactive vs non-interactive |
| Handle `merge continue` routing | ⬜ Not Started | |
| Handle `merge abort` routing | ⬜ Not Started | |
| Handle `merge status` routing | ⬜ Not Started | |
| Handle `merge complete` routing | ⬜ Not Started | |
| Implement exit code handling | ⬜ Not Started | |
| Auto-detect repo path if not specified | ⬜ Not Started | |
| Write CLI integration tests | ⬜ Not Started | |

### Files Created/Modified

- [ ] `src/bin/mergers.rs`

### Blockers

- Depends on Phase 4 completion

### Notes

- Repo path auto-detection: Check current directory for git repo
- If no state file found for continue/abort/status/complete, exit with code 4

---

## Phase 6: Interactive Mode Integration

**Status:** Not Started
**Estimated Effort:** Medium

### Tasks

| Task | Status | Notes |
|------|--------|-------|
| Update SetupRepoState to create state file | ⬜ Not Started | |
| Update CherryPickState to update state file | ⬜ Not Started | After each commit |
| Update ConflictResolutionState for state file | ⬜ Not Started | |
| Update CompletionState to mark ReadyForCompletion | ⬜ Not Started | |
| Update PostCompletionState to use core ops | ⬜ Not Started | |
| Add state file cleanup on TUI exit | ⬜ Not Started | Only if aborted |
| Test cross-mode resume (TUI → CLI) | ⬜ Not Started | |
| Test cross-mode resume (CLI → TUI) | ⬜ Not Started | Future? |

### Files Created/Modified

- [ ] `src/ui/state/default/setup_repo.rs`
- [ ] `src/ui/state/default/cherry_pick.rs`
- [ ] `src/ui/state/default/conflict_resolution.rs`
- [ ] `src/ui/state/default/completion.rs`
- [ ] `src/ui/state/default/post_completion.rs`

### Blockers

- Depends on Phase 1, 2 completion

### Notes

- State file should be updated on each state transition
- Lock should be held during TUI operation
- Lock released on TUI exit

---

## Phase 7: Testing & Documentation

**Status:** Not Started
**Estimated Effort:** Medium

### Tasks

| Task | Status | Notes |
|------|--------|-------|
| Write full workflow integration test | ⬜ Not Started | |
| Write conflict + continue integration test | ⬜ Not Started | |
| Write abort integration test | ⬜ Not Started | |
| Write complete integration test | ⬜ Not Started | |
| Write cross-mode resume test | ⬜ Not Started | |
| Update CLAUDE.md | ⬜ Not Started | Add new commands |
| Update README.md (if exists) | ⬜ Not Started | |
| Run full test suite | ⬜ Not Started | |
| Run clippy | ⬜ Not Started | |
| Run fmt | ⬜ Not Started | |

### Files Created/Modified

- [ ] `tests/` (integration tests)
- [ ] `.claude/CLAUDE.md`
- [ ] `README.md`

### Blockers

- Depends on all previous phases

### Notes

- Integration tests may need mocked API responses
- Use tempdir for worktree tests

---

## Session Log

### Session 1: 2024-12-22 - Planning

**Duration:** ~1 hour
**Activities:**
- Analyzed existing codebase structure
- Identified UI states and their logic
- Designed CLI subcommand structure
- Designed state file format
- Created implementation plan
- Created this progress document

**Decisions Made:**
- Subcommands instead of flags for special operations
- Per-repository state files with path hashing
- PID-based locking
- Keep all state files for history
- Both TUI and CLI generate state files
- Explicit `complete` command (no auto-tagging)

**Next Steps:**
- Start Phase 1: Foundation
- Create core module structure
- Implement state file handling

---

### Session 2: 2024-12-23 - Architecture Sync

**Duration:** ~30 minutes
**Activities:**
- Merged latest master with 3 new commits
- Reviewed new typed configuration architecture (PR #49)
- Reviewed git hooks flag feature (PR #46)
- Updated PLAN.md to document new architecture
- Updated PROGRESS.md with new tasks
- Updated VERIFICATION.md with new test cases

**Changes from Master:**
1. **AppMode trait with associated Config type** - `AppBase` is now generic `AppBase<C: AppModeConfig>`
2. **MergeConfig struct** - Contains `shared`, `work_item_state`, and `run_hooks`
3. **Git hooks flag** - `--run-hooks` CLI flag, disabled by default via `core.hooksPath=/dev/null`

**Impact on Plan:**
- Core operations should use `MergeConfig` directly (not `AppConfig` enum)
- State file must include `run_hooks` field for resume consistency
- Repository setup must pass `run_hooks` from config to git functions

**Next Steps:**
- Begin Phase 1: Foundation implementation
- Create core module structure
- Implement state file with run_hooks field

---

## Risks & Mitigation

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Large refactoring scope | High | Medium | Incremental changes, maintain backwards compat |
| State file format changes | Medium | Low | Use schema version for forward compat |
| Cross-platform lock issues | Medium | Low | Test on all platforms, use platform-specific APIs |
| Interactive mode regression | High | Low | Extensive testing, snapshot tests exist |

---

## Definition of Done

Each phase is considered done when:

1. All tasks are completed (✅)
2. Unit tests pass (`cargo nextest run`)
3. Clippy passes (`cargo clippy --all-targets --all-features -- -D warnings`)
4. Format is correct (`cargo fmt --check`)
5. Documentation is updated if needed
6. Code is committed to branch

Overall feature is done when:

1. All phases complete
2. Full integration tests pass
3. Cross-mode resume works
4. Documentation updated
5. PR ready for review
