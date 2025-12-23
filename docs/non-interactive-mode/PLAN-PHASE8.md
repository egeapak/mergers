# Phase 8: Lock File & State File Robustness

## Overview

This phase addresses two gaps identified in the implementation review:

1. **Lock File Management**: TUI mode does not acquire/release locks, only CLI does
2. **Corrupted State Handling**: No graceful handling of corrupted/invalid state files

## Design

### Lock File Requirements

| Requirement | Description |
|-------------|-------------|
| **Exclusive Access** | Only one merge operation per repository at a time |
| **Both Modes** | Lock must be acquired in BOTH TUI and CLI modes |
| **Clean Exit** | Lock must be released on normal exit |
| **Error Exit** | Lock must be released on error/panic |
| **Early Check** | CLI commands should check lock BEFORE loading state |
| **Clear Error** | Locked state should exit with code 7 and clear message |

### Corrupted State Requirements

| Requirement | Description |
|-------------|-------------|
| **Detection** | Detect JSON parse errors, missing fields, invalid values |
| **Clear Error** | Show specific error message about corruption |
| **Exit Code** | Exit with code 1 (GeneralError) for corrupted state |
| **No Crash** | Never panic on corrupted state files |
| **Suggestion** | Suggest `merge abort` or manual state file deletion |

---

## Implementation Plan

### 1. TUI Lock Management

**Files to modify:**
- `src/ui/apps/merge_app.rs` - Add lock acquisition

**Changes:**
1. Acquire lock in `create_state_file()` method (after state file creation)
2. Store `LockGuard` in MergeApp (already has `lock_guard: Option<LockGuard>`)
3. Lock is automatically released when MergeApp is dropped
4. If lock acquisition fails, show error in TUI and exit

**Implementation:**
```rust
// In create_state_file()
pub fn create_state_file(...) -> Result<()> {
    // ... create state file ...

    // Acquire lock
    match LockGuard::acquire(&repo_path) {
        Ok(Some(guard)) => {
            self.lock_guard = Some(guard);
        }
        Ok(None) => {
            return Err(anyhow::anyhow!("Another merge operation is in progress"));
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to acquire lock: {}", e));
        }
    }

    Ok(())
}
```

### 2. CLI Lock Early Check

**Files to modify:**
- `src/core/runner/non_interactive.rs` - Add early lock check

**Changes:**
1. Add lock check at start of `continue_merge()`, `abort()`, `status()`, `complete()`
2. For `run()`, lock is already acquired after repo setup
3. Return exit code 7 (Locked) if lock cannot be acquired

### 3. Corrupted State Handling

**Files to modify:**
- `src/core/state/file.rs` - Improve error messages
- `src/core/runner/non_interactive.rs` - Handle load errors gracefully

**Changes:**
1. Wrap state file loading in proper error handling
2. Distinguish between "file not found" and "file corrupted"
3. Provide actionable error messages:
   - "State file corrupted: {reason}. Run `merge abort` or delete {path}"
   - "State file has invalid schema version {v}. Expected {expected}"
   - "State file missing required field: {field}"

### 4. State File Validation

**Files to modify:**
- `src/core/state/file.rs` - Add validation method

**New method:**
```rust
impl MergeStateFile {
    pub fn validate(&self) -> Result<()> {
        // Check schema version
        if self.schema_version != SCHEMA_VERSION {
            bail!("Unsupported schema version: {}", self.schema_version);
        }

        // Check required fields
        if self.repo_path.as_os_str().is_empty() {
            bail!("Missing required field: repo_path");
        }

        // Check phase validity
        if self.phase == MergePhase::AwaitingConflictResolution
           && self.conflicted_files.is_none() {
            bail!("Invalid state: AwaitingConflictResolution but no conflicted_files");
        }

        // Check index bounds
        if self.current_index > self.cherry_pick_items.len() {
            bail!("Invalid state: current_index {} exceeds items count {}",
                  self.current_index, self.cherry_pick_items.len());
        }

        Ok(())
    }
}
```

---

## Test Plan

### Lock File Tests

| Test | Description |
|------|-------------|
| `test_tui_acquires_lock` | TUI acquires lock during state file creation |
| `test_tui_lock_blocks_cli` | CLI cannot start while TUI holds lock |
| `test_cli_lock_blocks_tui` | TUI shows error if CLI holds lock |
| `test_lock_released_on_tui_exit` | Lock file removed after normal TUI exit |
| `test_lock_released_on_tui_error` | Lock file removed even on TUI error |
| `test_lock_released_on_cli_exit` | Lock file removed after CLI command |
| `test_existing_lock_shows_error` | Clear error message when lock exists |
| `test_lock_exit_code_7` | CLI exits with code 7 when locked |

### Corrupted State Tests

| Test | Description |
|------|-------------|
| `test_corrupted_json_error` | Invalid JSON shows clear error |
| `test_missing_field_error` | Missing required field shows error |
| `test_invalid_schema_version` | Wrong schema version shows error |
| `test_invalid_phase_error` | Invalid phase value shows error |
| `test_invalid_index_error` | Out-of-bounds index shows error |
| `test_corrupted_state_exit_code_1` | Exits with code 1 on corruption |
| `test_corrupted_state_suggests_abort` | Error message suggests abort |

---

## Files to Create/Modify

### Modified Files
- `src/ui/apps/merge_app.rs` - TUI lock management
- `src/core/runner/non_interactive.rs` - CLI lock checks, corrupted state handling
- `src/core/state/file.rs` - Validation, improved error messages

### Test Files
- `tests/integration_tests.rs` - Add lock and corruption tests

---

## Exit Codes Reference

| Code | Name | When |
|------|------|------|
| 0 | Success | Operation completed |
| 1 | GeneralError | Corrupted state file, config errors |
| 2 | Conflict | Cherry-pick conflict |
| 3 | PartialSuccess | Some operations failed |
| 4 | NoStateFile | No state file found |
| 5 | InvalidPhase | Wrong phase for operation |
| 6 | NoPRsMatched | No PRs matched criteria |
| 7 | Locked | Another merge in progress |

---

## Verification Checklist

- [ ] TUI acquires lock on state file creation
- [ ] TUI releases lock on normal exit
- [ ] TUI releases lock on error exit
- [ ] CLI checks lock before loading state
- [ ] CLI shows clear error when locked
- [ ] CLI exits with code 7 when locked
- [ ] Corrupted state shows clear error
- [ ] Corrupted state exits with code 1
- [ ] Schema version mismatch detected
- [ ] Missing fields detected
- [ ] Invalid index detected
- [ ] All tests pass
