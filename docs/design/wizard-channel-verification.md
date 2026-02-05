# Channel-Based Wizard Step Execution - Verification Plan

## Overview

This document describes how to verify each phase of the channel-based wizard implementation is working correctly.

---

## Phase 1: Message Types and Context Extraction

### Automated Tests

```bash
# Run unit tests for context extraction
cargo test setup_context --lib

# Run with verbose output
cargo test setup_context --lib -- --nocapture
```

### Manual Verification

1. **Type Compilation**
   - Verify all new types compile without errors
   - Check `Send + 'static` bounds are satisfied for channel use

2. **Context Extraction**
   - In debug mode, print extracted context
   - Verify all fields populated correctly from `MergeApp`

### Acceptance Criteria

- [ ] `ProgressMessage` enum compiles and is `Send + 'static`
- [ ] `StepResult` enum compiles and is `Send + 'static`
- [ ] `SetupContext::from_app()` extracts all required data
- [ ] Unit tests pass for context extraction

---

## Phase 2: State Restructuring

### Automated Tests

```bash
# Run state tests
cargo test setup_repo --lib
```

### Manual Verification

1. **State Transitions**
   - `Idle` -> start -> `Running`
   - `Running` -> complete -> `Complete`
   - `Running` -> error -> `Error`
   - `Error` -> retry -> `Idle`

2. **UI Rendering**
   - Each state variant renders correctly
   - No panics on edge cases

### Acceptance Criteria

- [ ] `SetupState` enum has all required variants
- [ ] State transitions work as expected
- [ ] UI renders correctly for each state

---

## Phase 3: Background Task Implementation

### Automated Tests

```bash
# Run with mock API client
cargo test run_setup_task --lib

# Integration test
cargo test wizard_integration --lib
```

### Manual Verification

1. **Clone Mode Flow**
   - Start TUI without `--repo` flag
   - Observe all steps execute in sequence
   - Verify progress messages logged (if debug enabled)

2. **Worktree Mode Flow**
   - Start TUI with `--repo /path/to/repo`
   - Observe FetchDetails skipped
   - Verify FetchTargetBranch executes
   - Verify worktree created

3. **Step Timing**
   - Each step should start after previous completes
   - No race conditions or out-of-order messages

### Acceptance Criteria

- [ ] All 7 steps execute in correct order
- [ ] Clone mode skips worktree-specific steps
- [ ] Worktree mode skips clone-specific steps
- [ ] Errors properly reported through channel

---

## Phase 4: UI Integration

### Automated Tests

```bash
# Run snapshot tests
cargo test setup_repo --lib

# Update snapshots if needed
cargo insta review
```

### Manual Verification

1. **Progress Display**
   - Start TUI and observe wizard screen
   - Each step indicator updates as step starts/completes
   - Current step message updates

2. **Smooth Updates**
   - UI updates without flickering
   - Progress feels responsive (not waiting for 50ms ticks)

3. **Result Application**
   - After clone: `app.repo_path()` returns temp path
   - After worktree: `app.worktree.base_repo_path` set
   - After prepare: `app.cherry_pick_items()` non-empty

4. **State File Creation**
   - State file created at `~/.local/state/mergers/`
   - File contains correct paths and version

### Acceptance Criteria

- [ ] Progress indicators update in real-time
- [ ] All step results applied to `MergeApp`
- [ ] State file created successfully
- [ ] Transition to CherryPick state works

---

## Phase 5: Error Handling

### Automated Tests

```bash
# Run error handling tests
cargo test setup_repo_error --lib
```

### Manual Verification

1. **Branch Exists Error**
   - Create branch manually before running
   - Observe error displayed
   - Press 'f' to force delete and retry
   - Verify branch deleted and wizard proceeds

2. **Worktree Exists Error**
   - Create worktree manually before running
   - Observe error displayed
   - Press 'f' to force remove and retry
   - Verify worktree removed and wizard proceeds

3. **Network Error**
   - Disconnect network during FetchDetails
   - Observe error displayed
   - Reconnect and press 'r' to retry
   - Verify wizard proceeds

4. **Task Cancellation**
   - Start wizard
   - Press Esc during execution
   - Verify task aborted (no orphan processes)

### Acceptance Criteria

- [ ] Error state shows appropriate message
- [ ] 'r' retries from beginning
- [ ] 'f' force-resolves and retries
- [ ] Esc exits to error state
- [ ] Task properly aborted on exit

---

## Phase 6: Testing

### Automated Tests

```bash
# Run all setup_repo tests
cargo test setup_repo --lib

# Run with coverage
cargo llvm-cov nextest --test setup_repo

# Check for regressions
cargo test --all
```

### Manual Verification

1. **Snapshot Accuracy**
   - Review each snapshot file
   - Verify step indicators correct
   - Verify messages match step

2. **Async Test Reliability**
   - Run async tests multiple times
   - Verify no flaky failures

### Acceptance Criteria

- [ ] All snapshot tests pass
- [ ] All unit tests pass
- [ ] All async tests pass
- [ ] No flaky tests
- [ ] Coverage maintained or improved

---

## Integration Verification

### Full Flow Test - Clone Mode

```bash
# 1. Clean state
rm -rf ~/.local/state/mergers/

# 2. Run TUI without repo flag
cargo run -- merge

# 3. Observe:
#    - FetchDetails step executes
#    - CheckPrerequisites step executes
#    - CloneOrWorktree step executes (cloning)
#    - CreateBranch step executes
#    - PrepareCherryPicks step executes
#    - InitializeState step executes
#    - Transition to CherryPick screen

# 4. Verify:
#    - Temp directory created
#    - Branch created
#    - State file created
```

### Full Flow Test - Worktree Mode

```bash
# 1. Clean state
rm -rf ~/.local/state/mergers/

# 2. Run TUI with repo flag
cargo run -- merge --repo /path/to/local/repo

# 3. Observe:
#    - CheckPrerequisites step executes
#    - FetchTargetBranch step executes
#    - CloneOrWorktree step executes (worktree)
#    - CreateBranch step executes
#    - PrepareCherryPicks step executes
#    - InitializeState step executes
#    - Transition to CherryPick screen

# 4. Verify:
#    - Worktree created at repo/.worktrees/next-{version}
#    - Branch created
#    - State file created
```

### Comparison with Tick-Based Implementation

1. **Behavior Equivalence**
   - Both implementations should produce same end state
   - Same files/directories created
   - Same state file contents

2. **Performance**
   - Channel-based may be slightly faster (no 50ms waits)
   - Measure end-to-end time for comparison

3. **User Experience**
   - Progress updates should feel smoother
   - No perceptible difference in responsiveness

---

## Performance Benchmarks

### Targets

| Scenario | Steps | Max Total Time |
|----------|-------|----------------|
| Clone (network) | 7 | < 30s (depends on network) |
| Worktree (local) | 6 | < 5s |
| Error + Retry | 7 | < 35s |

### Measurement

```bash
# Run with timing
time cargo run -- merge 2>&1 | grep -E "Step|Complete"

# Or add instrumentation in background task
let start = std::time::Instant::now();
// ... step ...
tracing::debug!("Step {:?} took {:?}", step, start.elapsed());
```

---

## Regression Tests

After implementation, ensure these pass in CI:

```yaml
# .github/workflows/ci.yml
- name: Run wizard tests
  run: |
    cargo test setup_repo --lib
    cargo test wizard --lib
```

---

## Known Limitations

1. **No Step Timeout**: Individual steps can hang indefinitely
2. **No Progress Percentage**: Steps show started/completed, not percentage
3. **No Cancel During Step**: Can only cancel between steps

---

## Rollback Plan

If issues arise:

1. **Revert to Tick-Based**
   - `git checkout HEAD~1 -- src/ui/state/default/setup_repo.rs`
   - Update snapshots
   - No other files affected

2. **Hybrid Approach**
   - Keep channel architecture
   - Add fallback to tick-based for problematic steps

3. **Feature Flag**
   - Add `--use-channel-wizard` flag
   - Default to tick-based
   - Gather feedback before switching default

---

## Comparison Checklist

Before considering implementation complete, verify:

| Aspect | Tick-Based | Channel-Based |
|--------|------------|---------------|
| Clone mode works | ✅ | ⬜ |
| Worktree mode works | ✅ | ⬜ |
| Error display | ✅ | ⬜ |
| Retry works | ✅ | ⬜ |
| Force-resolve works | ✅ | ⬜ |
| State file created | ✅ | ⬜ |
| All tests pass | ✅ | ⬜ |
| Snapshots correct | ✅ | ⬜ |
