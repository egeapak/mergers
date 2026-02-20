# Mergers Project Review

**Date:** 2026-02-20
**Scope:** Full codebase review covering architecture, correctness, security, testing

## Overview

The project is a well-structured Rust CLI/TUI application (~50K+ lines across 76+ source files) with 1,047 passing tests. Architecture is solid with clear module separation, type-safe state machines, comprehensive snapshot testing (149 snapshots), and multi-platform CI. The deep review uncovered several bugs and gaps ranging from critical to low severity.

---

## Critical Findings

### 1. Shallow clone mode is fundamentally broken

**Location:** `src/core/runner/merge_engine.rs:222-235`

The `TempDir` returned by `shallow_clone_repo()` is dropped when `setup_repository()` returns, which deletes the cloned directory. The returned `clone_path` becomes a dangling path. All subsequent operations fail with "No such file or directory." The code comment acknowledges the behavior. The non-worktree code path (when no `--local-repo` is specified) is completely unusable.

### 2. No signal handling anywhere in the codebase

There are zero `SIGINT`/`SIGTERM`/`ctrlc` handlers. For a CI/CD-oriented tool:
- `Ctrl+C` during cherry-pick processing leaves the repository in a dirty state
- `process::exit()` in `bin/mergers.rs:108` does not run destructors, so `LockGuard::Drop` never executes
- State is not saved incrementally during the cherry-pick loop, so a kill loses all progress
- Container orchestrators sending `SIGTERM` before `SIGKILL` get no graceful shutdown

---

## High Severity Findings

### 3. Double lock acquisition causes self-deadlock in `run()`

**Location:** `src/core/runner/non_interactive.rs:157` + `src/core/state/manager.rs:129`

The `run()` method acquires a lock at line 157, then calls `engine.create_state_file()` at line 229, which internally calls `LockGuard::acquire()` again. The second acquisition finds the lock file contains the current PID, determines the process is alive, and returns `Ok(None)` ("another process holds the lock"). This causes `create_state_file()` to fail. **The `run()` method will always fail in production.** Tests pass because they call `create_state_file()` directly without the outer lock.

### 4. `continue_merge` never calls `git cherry-pick --continue`

**Location:** `src/core/runner/non_interactive.rs:360-364`

After verifying conflicts are resolved, the code marks the current item as `Success` and advances the index, but never runs `git cherry-pick --continue` to finalize the commit. The function `continue_cherry_pick` exists in `src/git.rs:631` and is exposed on the trait, but is never called from any consumer. The repository remains in a mid-cherry-pick state.

### 5. No API request timeouts

**Location:** `src/api/client.rs:93-94`

The `reqwest::ClientBuilder` is constructed with no timeout parameters. `reqwest` defaults to no timeout, so any API call can hang indefinitely.

### 6. No API retry logic (parameter is silently discarded)

**Location:** `src/api/client.rs:113-138`

The `max_retries` parameter accepted by constructors is silently discarded (`#[allow(unused_variables)]`). The `max_retries()` accessor returns a hardcoded `3` that controls nothing. Transient network errors cause immediate failure.

### 7. TOCTOU race condition in lock acquisition

**Location:** `src/core/state/file.rs:770-807`

`LockGuard::acquire()` checks if the lock file exists, reads the PID, checks if the process is alive, then writes a new lock file. None of this is atomic. Two processes detecting a stale lock simultaneously could both create their own lock file. Using `O_EXCL` or `flock()` would fix this.

---

## Medium Severity Findings

### 8. State not persisted during cherry-pick loop

**Location:** `src/core/runner/merge_engine.rs` `process_cherry_picks()`

Item statuses are updated in memory only. The disk save happens afterward. A kill mid-loop loses all progress.

### 9. No rate limit handling for Azure DevOps API

**Location:** `src/api/client.rs`

Parallel fetching via `buffer_unordered` can generate many simultaneous requests. `ApiError::RateLimited` exists but is never constructed or checked. HTTP 429 with `Retry-After` headers is ignored.

### 10. Work item batch size not chunked

**Location:** `src/api/client.rs`

Both `fetch_work_items_for_pr` and `fetch_work_items_by_ids` send all IDs in a single request. Azure DevOps has a 200-ID-per-request limit.

### 11. Silent error swallowing in API client

**Location:** `src/api/client.rs:482, 528, 568`

Three locations discard API errors without logging.

### 12. `validate_git_ref` used inconsistently

**Location:** `src/git.rs`

Called in only 2 of 17+ public functions. Does not reject leading `-` characters. No `--` end-of-options separator in any of ~50 git command invocations.

### 13. `get_commit_info` uses `|` as delimiter

**Location:** `src/git.rs:685-707`

Format string `%H|%ci|%s|%an` is parsed by splitting on `|`. Commit messages containing `|` corrupt the parsed fields.

### 14. No formal phase transition validation

**Location:** `src/core/state/` across multiple files

Phase changes are direct assignments with no transition validation function.

### 15. 3-digit hex colors parsed incorrectly

**Location:** `src/utils/html_parser.rs`

`#F53` is parsed as `RGB(0, 15, 83)` instead of CSS-correct `RGB(255, 85, 51)`. The test documents the wrong behavior.

### 16. `all_work_items_terminal` carries dual semantics

**Location:** `src/migration.rs:64-70`

When a PR has zero work items, the field is set to `true` (meaning "check skipped"), same as when all items are actually terminal. Consumers can't distinguish these cases.

---

## Low Severity Findings

| # | Finding | Location |
|---|---------|----------|
| 17 | `force_remove_worktree`, `force_delete_branch`, `abort_cherry_pick` silently discard all errors | `git.rs:418, 444, 664` |
| 18 | Inconsistent use of `git_command()` helper (~50% bypass it) | `git.rs` |
| 19 | `.to_str().unwrap()` on paths can panic on non-UTF-8 | `git.rs:234, 300` |
| 20 | `parse_patch_branch` splits on wrong hyphen for multi-hyphen branch names | `git.rs:1111` |
| 21 | `Throttler::new(0)` deadlocks (no input validation) | `utils/throttle.rs` |
| 22 | `Semaphore::acquire()` result silently ignored | `utils/throttle.rs:26` |
| 23 | `_max_concurrent_processing` parameter accepted but unused | `utils/throttle.rs:43` |
| 24 | `MergeArgs`, `MigrateArgs`, `CleanupArgs` missing `Debug` derive | `models.rs:319, 342, 357` |
| 25 | `CherryPickStatus`/`CleanupStatus` missing `PartialEq`/`Display` | `models.rs:1414, 1469` |
| 26 | PAT field is `pub` with no Debug redaction | `models.rs:797` |
| 27 | `categorize_prs` returns `Result` but never errors | `migration.rs:190` |
| 28 | Unnecessary `.clone()` on `unsure_details` | `migration.rs:250` |
| 29 | `Loading` and `Setup` phases are dead code | `MergePhase` enum |
| 30 | `run_hooks` config carried through all layers but never implemented | `cherry_pick.rs:195-196` |
| 31 | `abort()` returns exit code 0 (indistinguishable from success) | `non_interactive.rs` |
| 32 | No stale state file cleanup / no orphaned `.json.tmp` cleanup | State directory |
| 33 | Misleading backward-compat constructors accept `pool_idle_timeout_secs`/`max_retries` but ignore them | `api/client.rs` |

---

## Test Coverage Assessment

### Current State

- **1,016 unit tests** + **31 integration tests** + **149 snapshot tests** = **1,047 total**
- All passing (0 failures)
- Strong UI snapshot coverage across all three modes (merge, migration, cleanup)
- Good test infrastructure: `TuiTestHarness`, 6 config builders, 10+ data builders
- Integration tests cover config loading, state file lifecycle, lock mechanisms
- Benchmark for dependency analysis performance

### Gaps

1. **No E2E tests for non-interactive merge workflow.** `run()`, `continue_merge()`, `abort()`, `complete()`, and `status()` are not tested as integrated flows. This is why the double-lock bug (#3) and missing cherry-pick continue (#4) went undetected.

2. **No integration tests with a real git repository for the merge engine.** Git operations have 52 unit tests with temp repos, but the full flow (setup → cherry-pick → conflict → continue → complete) is never exercised end-to-end.

3. **No API integration tests with HTTP mocking.** `mockito` is a dev dependency and `api/client.rs` has 45 tests, but the full data-loading pipeline is untested as an integrated flow.

4. **No tests for `bin/mergers.rs` entry point.** CLI argument parsing, config resolution, and exit-code translation are untested.

5. **`unsafe` env var manipulation in model tests without `#[serial]`.** Six `unsafe` blocks risk flaky failures under parallel execution.

6. **No adversarial tests for git input validation.** Branch names with leading dashes, special characters, or excessive length are not tested.

---

## Recommended New Tests

### E2E Integration Tests (highest value)

#### Non-interactive merge workflow E2E
```
Setup: Real temp git repo with commits on a dev branch
Flow: run() → cherry-pick succeeds → complete()
Verify: State file lifecycle, exit codes, git history
```

#### Merge with conflict E2E
```
Setup: Temp repo with conflicting commits
Flow: run() → conflict detected (exit 2) → manual resolution → continue_merge() → complete()
Verify: cherry-pick --continue called, state transitions correct
```

#### Abort E2E
```
Setup: Start a merge, then abort
Flow: run() → abort()
Verify: Worktree cleaned up, state marked aborted, git state clean
```

#### Status command E2E
```
Setup: State files in various phases
Flow: status() with each output format (text, json, ndjson)
Verify: Exit codes match phase, output parses correctly
```

### API Integration Tests

#### Full data loading pipeline with mockito
```
Setup: Mock Azure DevOps endpoints for PRs, work items, history
Flow: Load PRs → fetch work items → analyze dependencies
Verify: Correct data assembly, error propagation, pagination
```

#### Rate limiting and retry behavior
```
Setup: Mock returning 429 with Retry-After header
Verify: Retry occurs after delay (once retry logic is implemented)
```

### Git Operation Tests

#### Adversarial git ref validation
```
Input: Branch names with leading dashes, pipes, null bytes, spaces
Verify: Properly rejected before git command execution
```

#### Commit info parsing with special characters
```
Input: Commit messages containing | characters, multi-line, unicode
Verify: Correct field extraction
```

### State Management Tests

#### Lock acquisition under contention
```
Setup: Two processes trying to acquire the same lock
Verify: Exactly one succeeds
```

#### State file corruption recovery (extend existing)
```
Input: Truncated JSON, invalid schema version, out-of-bounds index
Verify: Graceful error with recovery instructions
```
