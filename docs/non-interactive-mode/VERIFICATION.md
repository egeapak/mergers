# Non-Interactive Merge Mode - Verification Checklist

## Overview

This document provides comprehensive verification checklists for testing the non-interactive merge mode implementation.

**Branch:** `claude/add-noninteractive-merge-mode-nGPeX`
**Last Updated:** 2024-12-23

---

## Table of Contents

1. [Unit Test Checklist](#unit-test-checklist)
2. [Integration Test Checklist](#integration-test-checklist)
3. [Manual Test Scenarios](#manual-test-scenarios)
4. [Edge Cases](#edge-cases)
5. [Regression Tests](#regression-tests)
6. [Performance Verification](#performance-verification)
7. [Platform Compatibility](#platform-compatibility)

---

## Unit Test Checklist

### State File (`src/core/state/file.rs`)

| Test | Description | Status |
|------|-------------|--------|
| `test_state_file_serialization` | MergeStateFile serializes to JSON correctly | ⬜ |
| `test_state_file_deserialization` | MergeStateFile deserializes from JSON correctly | ⬜ |
| `test_state_file_round_trip` | Serialize → Deserialize produces identical struct | ⬜ |
| `test_path_hashing_consistent` | Same path produces same hash | ⬜ |
| `test_path_hashing_different` | Different paths produce different hashes | ⬜ |
| `test_path_hashing_canonical` | Canonicalization works (./foo == foo) | ⬜ |
| `test_state_dir_default` | Default state dir is XDG compliant | ⬜ |
| `test_state_dir_env_override` | MERGERS_STATE_DIR overrides default | ⬜ |
| `test_path_for_repo` | Correct path generated for repo | ⬜ |
| `test_lock_path_for_repo` | Correct lock path generated for repo | ⬜ |
| `test_schema_version` | Schema version is set correctly | ⬜ |
| `test_phase_serialization` | All MergePhase variants serialize | ⬜ |
| `test_status_serialization` | All MergeStatus variants serialize | ⬜ |
| `test_item_status_serialization` | All StateItemStatus variants serialize | ⬜ |
| `test_run_hooks_serialization` | run_hooks field serializes correctly | ⬜ |
| `test_run_hooks_defaults_false` | run_hooks defaults to false when missing | ⬜ |

### Lock Guard (`src/core/state/file.rs`)

| Test | Description | Status |
|------|-------------|--------|
| `test_acquire_lock_creates_file` | Lock file created with PID | ⬜ |
| `test_acquire_lock_blocks_second` | Second lock acquisition fails | ⬜ |
| `test_lock_released_on_drop` | Lock file removed when LockGuard dropped | ⬜ |
| `test_stale_lock_detected` | Stale lock (dead PID) is detected | ⬜ |
| `test_stale_lock_removed` | Stale lock file is removed | ⬜ |
| `test_lock_content_is_pid` | Lock file contains current PID | ⬜ |

### PR Selection (`src/core/operations/pr_selection.rs`)

| Test | Description | Status |
|------|-------------|--------|
| `test_filter_empty_prs` | Empty PR list returns empty result | ⬜ |
| `test_filter_no_matching_states` | No PRs match returns empty | ⬜ |
| `test_filter_all_match` | All PRs match returns all | ⬜ |
| `test_filter_partial_match` | Some PRs match returns those | ⬜ |
| `test_filter_case_insensitive` | State matching is case-insensitive | ⬜ |
| `test_filter_pr_without_work_items_excluded` | PRs without WIs excluded | ⬜ |
| `test_filter_all_wis_must_match` | PR with one non-matching WI excluded | ⬜ |
| `test_filter_multiple_states` | Multiple allowed states work | ⬜ |
| `test_filter_preserves_order` | Original PR order preserved | ⬜ |
| `test_select_marks_selected` | In-place selection sets selected=true | ⬜ |
| `test_select_returns_count` | Returns correct count of selected | ⬜ |

### State Conversion (`src/core/state/conversion.rs`)

| Test | Description | Status |
|------|-------------|--------|
| `test_cherry_pick_item_to_state` | CherryPickItem → StateCherryPickItem | ⬜ |
| `test_state_to_cherry_pick_item` | StateCherryPickItem → CherryPickItem | ⬜ |
| `test_status_pending_conversion` | Pending status converts correctly | ⬜ |
| `test_status_success_conversion` | Success status converts correctly | ⬜ |
| `test_status_conflict_conversion` | Conflict status converts correctly | ⬜ |
| `test_status_failed_conversion` | Failed status with message converts | ⬜ |
| `test_status_skipped_conversion` | Skipped status converts correctly | ⬜ |
| `test_round_trip_preserves_data` | Full round trip preserves all data | ⬜ |

### Output Events (`src/core/output/events.rs`)

| Test | Description | Status |
|------|-------------|--------|
| `test_start_event_serialization` | Start event JSON is correct | ⬜ |
| `test_cherry_pick_start_serialization` | CherryPickStart JSON is correct | ⬜ |
| `test_cherry_pick_success_serialization` | CherryPickSuccess JSON is correct | ⬜ |
| `test_cherry_pick_conflict_serialization` | CherryPickConflict JSON is correct | ⬜ |
| `test_cherry_pick_failed_serialization` | CherryPickFailed JSON is correct | ⬜ |
| `test_complete_event_serialization` | Complete event JSON is correct | ⬜ |
| `test_event_has_event_field` | All events have "event" tag field | ⬜ |

### Output Formatters (`src/core/output/format.rs`)

| Test | Description | Status |
|------|-------------|--------|
| `test_text_formatter_progress` | Text progress is readable | ⬜ |
| `test_text_formatter_conflict` | Text conflict output is clear | ⬜ |
| `test_text_formatter_summary` | Text summary is informative | ⬜ |
| `test_json_formatter_valid` | JSON output is valid JSON | ⬜ |
| `test_ndjson_one_per_line` | NDJSON has one object per line | ⬜ |
| `test_ndjson_each_line_valid` | Each NDJSON line is valid JSON | ⬜ |

---

## Integration Test Checklist

### Full Workflow Tests

| Test | Description | Status |
|------|-------------|--------|
| `test_non_interactive_full_success` | Complete merge without conflicts | ⬜ |
| `test_non_interactive_with_conflict` | Merge with conflict, exit code 2 | ⬜ |
| `test_conflict_then_continue` | Conflict → resolve → continue → success | ⬜ |
| `test_conflict_then_abort` | Conflict → abort → cleaned up | ⬜ |
| `test_complete_after_ready` | Cherry-picks done → complete → tagged | ⬜ |
| `test_abort_during_cherry_pick` | Abort mid-process cleans up | ⬜ |
| `test_status_shows_correct_phase` | Status reflects actual state | ⬜ |

### State Persistence Tests

| Test | Description | Status |
|------|-------------|--------|
| `test_state_file_created_on_run` | State file exists after run starts | ⬜ |
| `test_state_file_updated_on_progress` | State file updated after each commit | ⬜ |
| `test_state_file_contains_all_items` | All cherry-pick items in state | ⬜ |
| `test_state_file_survives_restart` | Can load state after process restart | ⬜ |
| `test_state_file_phase_correct` | Phase reflects current operation | ⬜ |
| `test_state_file_run_hooks_persisted` | run_hooks value is saved in state file | ⬜ |
| `test_continue_uses_saved_run_hooks` | Continue respects saved run_hooks setting | ⬜ |

### Git Hooks Tests

| Test | Description | Status |
|------|-------------|--------|
| `test_hooks_disabled_by_default` | Worktree has core.hooksPath=/dev/null by default | ⬜ |
| `test_hooks_enabled_with_flag` | Worktree has hooks enabled when --run-hooks used | ⬜ |
| `test_run_hooks_passed_to_setup` | run_hooks from config reaches git functions | ⬜ |
| `test_continue_preserves_hooks_setting` | Continue uses saved run_hooks, not default | ⬜ |

### Lock Tests

| Test | Description | Status |
|------|-------------|--------|
| `test_concurrent_merge_blocked` | Second merge fails with lock error | ⬜ |
| `test_lock_released_after_run` | Lock released when command completes | ⬜ |
| `test_lock_released_on_error` | Lock released even on error | ⬜ |
| `test_stale_lock_allows_new_merge` | Dead process lock doesn't block | ⬜ |

### Cross-Mode Tests

| Test | Description | Status |
|------|-------------|--------|
| `test_tui_creates_state_file` | TUI mode creates state file | ⬜ |
| `test_cli_continue_after_tui_conflict` | Can continue with CLI after TUI conflict | ⬜ |
| `test_cli_complete_after_tui_done` | Can complete with CLI after TUI cherry-picks | ⬜ |
| `test_cli_abort_after_tui_conflict` | Can abort with CLI after TUI conflict | ⬜ |

### Exit Code Tests

| Test | Description | Status |
|------|-------------|--------|
| `test_exit_code_0_on_success` | Full success returns 0 | ⬜ |
| `test_exit_code_1_on_general_error` | Config error returns 1 | ⬜ |
| `test_exit_code_2_on_conflict` | Conflict returns 2 | ⬜ |
| `test_exit_code_3_on_partial` | Partial success returns 3 | ⬜ |
| `test_exit_code_4_no_state_file` | Continue without state returns 4 | ⬜ |
| `test_exit_code_5_invalid_phase` | Wrong phase for operation returns 5 | ⬜ |
| `test_exit_code_6_no_prs` | No PRs match returns 6 | ⬜ |
| `test_exit_code_7_locked` | Already locked returns 7 | ⬜ |

---

## Manual Test Scenarios

### Scenario 1: Basic Non-Interactive Merge

**Preconditions:**
- Azure DevOps project with merged PRs
- PRs have work items in "Ready for Next" state
- Local repo or worktree capability

**Steps:**
1. Run `mergers merge run -n --version v1.0.0 --select-by-state "Ready for Next"`
2. Observe progress output
3. Check exit code
4. Verify worktree has hooks disabled (core.hooksPath=/dev/null)

**Expected Results:**
- [ ] Progress events shown (or JSON if specified)
- [ ] State file created
- [ ] State file contains `run_hooks: false`
- [ ] Exit code 0 (or 2 if conflict)
- [ ] Worktree created with cherry-picked commits
- [ ] Worktree has hooks disabled by default

### Scenario 1b: Non-Interactive Merge with Hooks Enabled

**Preconditions:**
- Same as Scenario 1

**Steps:**
1. Run `mergers merge run -n --version v1.0.0 --select-by-state "Ready for Next" --run-hooks`
2. Check state file and worktree

**Expected Results:**
- [ ] State file contains `run_hooks: true`
- [ ] Worktree does NOT have core.hooksPath set to /dev/null

### Scenario 2: Conflict Resolution Flow

**Preconditions:**
- PRs with known conflicting changes

**Steps:**
1. Run merge that will conflict
2. Verify exit code is 2
3. Navigate to worktree
4. Resolve conflicts manually
5. Stage resolved files (`git add .`)
6. Run `mergers merge continue`
7. Repeat if more conflicts
8. Verify successful completion

**Expected Results:**
- [ ] Conflict detected and reported
- [ ] Conflicted files listed in output
- [ ] State file phase is `AwaitingConflictResolution`
- [ ] Continue resumes from correct commit
- [ ] Final exit code is 0 or 3

### Scenario 3: Abort Flow

**Preconditions:**
- In-progress merge (running or after conflict)

**Steps:**
1. Start a merge
2. Interrupt or cause conflict
3. Run `mergers merge abort`
4. Verify cleanup

**Expected Results:**
- [ ] Worktree removed (if applicable)
- [ ] Branch deleted
- [ ] Cherry-pick aborted
- [ ] State file phase is `Aborted`
- [ ] Lock released

### Scenario 4: Complete Flow

**Preconditions:**
- Merge in `ReadyForCompletion` phase

**Steps:**
1. Complete cherry-picks (no conflicts or all resolved)
2. Verify phase is `ReadyForCompletion`
3. Push branch manually
4. Run `mergers merge complete --next-state "Done"`
5. Verify PR tags and WI updates

**Expected Results:**
- [ ] All successful PRs tagged in Azure DevOps
- [ ] All work items updated to "Done" state
- [ ] State file phase is `Completed`
- [ ] Exit code is 0

### Scenario 5: Status Check

**Preconditions:**
- In-progress merge at various phases

**Steps:**
1. Run `mergers merge status --output json`
2. Check for each phase

**Expected Results:**
- [ ] Phase correctly reported
- [ ] Progress (completed/total) shown
- [ ] Conflicted files listed if applicable
- [ ] Repo path shown

### Scenario 6: Interactive to CLI Handoff

**Preconditions:**
- Repository configured for TUI mode

**Steps:**
1. Start `mergers merge` (TUI)
2. Select PRs, enter version
3. Let cherry-pick start
4. Exit TUI on conflict (Ctrl+C or abort)
5. Resolve conflicts in terminal
6. Run `mergers merge continue`
7. Complete merge

**Expected Results:**
- [ ] TUI creates state file
- [ ] State file has correct progress
- [ ] CLI can read TUI's state file
- [ ] Continue works correctly
- [ ] Complete works correctly

---

## Edge Cases

### PR Selection Edge Cases

| Case | Expected Behavior | Status |
|------|-------------------|--------|
| PR with 0 work items | Excluded from selection | ⬜ |
| PR with 1 WI matching | Selected | ⬜ |
| PR with 2 WIs, 1 matching | Excluded (ALL must match) | ⬜ |
| PR with 2 WIs, 2 matching | Selected | ⬜ |
| WI with null state field | Treated as non-matching | ⬜ |
| Empty select-by-state list | Error (required argument) | ⬜ |
| State with spaces | Properly matched | ⬜ |
| State case mismatch | Case-insensitive match | ⬜ |

### State File Edge Cases

| Case | Expected Behavior | Status |
|------|-------------------|--------|
| State file corrupted | Error with clear message | ⬜ |
| State file missing | Error code 4 for continue/complete | ⬜ |
| State file wrong schema version | Warning or error | ⬜ |
| State dir doesn't exist | Created automatically | ⬜ |
| State dir not writable | Clear error message | ⬜ |
| Repo path with spaces | Handled correctly | ⬜ |
| Repo path with unicode | Handled correctly | ⬜ |

### Lock Edge Cases

| Case | Expected Behavior | Status |
|------|-------------------|--------|
| Lock file corrupted | Treated as stale, removed | ⬜ |
| Lock file empty | Treated as stale, removed | ⬜ |
| Lock file non-numeric | Treated as stale, removed | ⬜ |
| Process crashed (lock orphaned) | Stale detection works | ⬜ |
| Lock file not writable | Clear error message | ⬜ |

### Git Operation Edge Cases

| Case | Expected Behavior | Status |
|------|-------------------|--------|
| Cherry-pick of empty commit | `--allow-empty` handles it | ⬜ |
| Cherry-pick of merge commit | `-m 1` handles it | ⬜ |
| Commit already applied | Handled gracefully | ⬜ |
| Worktree already exists | Clear error, option to force | ⬜ |
| Branch already exists | Clear error, option to force | ⬜ |
| Repository not found | Clear error message | ⬜ |
| Not in a git repository | Clear error message | ⬜ |

### Hooks Edge Cases

| Case | Expected Behavior | Status |
|------|-------------------|--------|
| Start with --run-hooks, continue without flag | Uses saved setting (hooks enabled) | ⬜ |
| Start without --run-hooks, continue with flag | Uses saved setting (hooks disabled) | ⬜ |
| State file missing run_hooks field | Defaults to false (hooks disabled) | ⬜ |
| Clone repo with --run-hooks | Hooks remain enabled | ⬜ |
| Worktree with --run-hooks | Hooks remain enabled | ⬜ |

### API Edge Cases

| Case | Expected Behavior | Status |
|------|-------------------|--------|
| PAT expired | Clear auth error | ⬜ |
| PAT insufficient permissions | Clear permission error | ⬜ |
| Network timeout | Retry or clear error | ⬜ |
| PR not found | Skip with warning | ⬜ |
| Work item not found | Skip with warning | ⬜ |
| Rate limited | Retry with backoff or error | ⬜ |

---

## Regression Tests

### Existing Functionality

| Test | Description | Status |
|------|-------------|--------|
| Interactive merge still works | TUI merge unchanged | ⬜ |
| Migration mode unchanged | Migration mode works | ⬜ |
| Cleanup mode unchanged | Cleanup mode works | ⬜ |
| Config loading unchanged | All config sources work | ⬜ |
| Git operations unchanged | Cherry-pick, cleanup work | ⬜ |
| API operations unchanged | Fetch PRs, WIs work | ⬜ |
| Existing tests pass | `cargo nextest run` | ⬜ |
| Clippy clean | No new warnings | ⬜ |
| Format correct | `cargo fmt --check` | ⬜ |
| Hooks flag works in TUI | --run-hooks respected in interactive mode | ⬜ |
| Typed config works | MergeConfig access is type-safe | ⬜ |

### Snapshot Tests

| Test | Description | Status |
|------|-------------|--------|
| All UI snapshots unchanged | No visual regressions | ⬜ |
| New snapshots for any UI changes | Captured and reviewed | ⬜ |

---

## Performance Verification

| Test | Expected | Actual | Status |
|------|----------|--------|--------|
| State file save < 50ms | | | ⬜ |
| State file load < 50ms | | | ⬜ |
| Lock acquire < 10ms | | | ⬜ |
| PR filtering (100 PRs) < 10ms | | | ⬜ |
| Hash computation < 1ms | | | ⬜ |

---

## Platform Compatibility

### Linux

| Test | Status |
|------|--------|
| State dir uses XDG | ⬜ |
| Lock with libc::kill | ⬜ |
| All tests pass | ⬜ |
| CLI works correctly | ⬜ |

### macOS

| Test | Status |
|------|--------|
| State dir uses XDG equivalent | ⬜ |
| Lock with libc::kill | ⬜ |
| All tests pass | ⬜ |
| CLI works correctly | ⬜ |

### Windows

| Test | Status |
|------|--------|
| State dir uses AppData | ⬜ |
| Lock with Windows API | ⬜ |
| All tests pass | ⬜ |
| CLI works correctly | ⬜ |
| Path handling correct | ⬜ |

---

## Verification Sign-off

### Phase 1 Sign-off

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Manual testing complete
- [ ] Edge cases verified
- [ ] Regression tests pass
- [ ] Reviewed by: _______________
- [ ] Date: _______________

### Phase 2 Sign-off

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Manual testing complete
- [ ] Edge cases verified
- [ ] Regression tests pass
- [ ] Reviewed by: _______________
- [ ] Date: _______________

### Phase 3 Sign-off

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Manual testing complete
- [ ] Edge cases verified
- [ ] Regression tests pass
- [ ] Reviewed by: _______________
- [ ] Date: _______________

### Phase 4 Sign-off

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Manual testing complete
- [ ] Edge cases verified
- [ ] Regression tests pass
- [ ] Reviewed by: _______________
- [ ] Date: _______________

### Phase 5 Sign-off

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Manual testing complete
- [ ] Edge cases verified
- [ ] Regression tests pass
- [ ] Reviewed by: _______________
- [ ] Date: _______________

### Phase 6 Sign-off

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Manual testing complete
- [ ] Edge cases verified
- [ ] Regression tests pass
- [ ] Reviewed by: _______________
- [ ] Date: _______________

### Phase 7 Sign-off

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Manual testing complete
- [ ] Edge cases verified
- [ ] Regression tests pass
- [ ] Reviewed by: _______________
- [ ] Date: _______________

### Final Sign-off

- [ ] All phases complete
- [ ] Full test suite passes
- [ ] Documentation updated
- [ ] Ready for PR
- [ ] Approved by: _______________
- [ ] Date: _______________
