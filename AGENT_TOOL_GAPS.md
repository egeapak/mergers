# Non-Interactive Mode: Agent Readiness Gaps

Analysis of the `mergers merge -n` non-interactive mode for autonomous AI agent usage.

## Summary

The non-interactive mode is well-designed for scripted/CI use with structured exit codes (0-7), JSON/NDJSON output, and state file persistence. However, several gaps prevent fully autonomous agent operation.

## P0 - Correctness Bug

### `continue` doesn't finalize the cherry-pick

**Files:** `src/core/runner/non_interactive.rs:353-382`, `src/core/runner/merge_engine.rs:333-410`

When a conflict occurs, the flow pauses and tells the user to resolve conflicts and run `merge continue`. The continue flow:

1. Checks conflicts are resolved via `git ls-files -u`
2. Marks the current item as `Success` and increments `current_index`
3. Calls `process_cherry_picks()` to continue with the next PR

**Missing step:** Nobody calls `git cherry-pick --continue` (or `git commit --no-edit`) to finalize the resolved conflict. The function `continue_cherry_pick()` exists in `src/git.rs:631` and handles both empty and non-empty commits, but it is never called by `continue_merge()`. The resolved files are staged but the cherry-pick is left in an unfinished state, which means the next `cherry_pick_commit()` call may fail or produce unexpected results.

**Fix:** Call `git::continue_cherry_pick(&state.repo_path)` (or the equivalent via the git operations trait) after verifying conflicts are resolved and before marking the item as `Success`.

## P0 - State File Path Not in JSON Output

**File:** `src/bin/mergers.rs:104-106`

The state file path is printed to **stderr** via `handle_run_result()`:
```rust
if let Some(ref path) = result.state_file_path {
    eprintln!("State file: {}", path.display());
}
```

This is not included in the JSON/NDJSON stdout output. An agent parsing structured JSON output won't discover the state file path unless it also parses stderr. The state file path should be included in the `Start` event or as a dedicated event in the structured output stream.

## P1 - No `merge skip` Command

When an agent encounters a conflict it cannot resolve, the only options are:
- `merge continue` (requires conflict resolution)
- `merge abort` (abandons the entire operation)

A `merge skip` command is needed that:
- Runs `git cherry-pick --abort` for the current PR
- Marks the current cherry-pick item as `Skipped`
- Advances `current_index`
- Continues processing remaining PRs

## P1 - Error Codes Always None in JSON Events

**File:** `src/core/runner/non_interactive.rs:780-788`

```rust
fn emit_error(&mut self, message: &str) {
    let event = ProgressEvent::Error {
        message: message.to_string(),
        code: None,  // Always None
    };
```

The `ProgressEvent::Error` has an `Option<String>` code field but it's never populated. Agents need machine-parseable error codes (e.g., `auth_failed`, `rate_limited`, `conflict`, `network_error`, `locked`) to make automated decisions without parsing free-text error messages.

## P1 - No `merge list` / PR Query Command

Agents cannot discover available PRs without starting a merge. A `merge list` command that shows PRs matching criteria (by work item state, date range, etc.) and their dependency analysis without initiating any operation would let agents gather information before deciding to proceed.

## P1 - No `--select-by-pr-id` for Targeted Operations

`--select-by-state` filters by work item state, but agents often need to cherry-pick specific PRs by ID. A `--select-by-pr-id 123,456,789` flag would enable targeted operations.

## P2 - Conflict Resolution Guidance Too Generic

**File:** `src/core/output/events.rs:207-213`

`ConflictInfo` provides static resolution instructions but lacks:
- The actual conflict diff content (the `<<<<<<<`/`>>>>>>>` markers)
- The original PR description/context to understand intent
- Which side of the conflict (ours vs theirs) each hunk represents

For agents to auto-resolve conflicts, the conflict event should include the raw diff or at minimum the conflicted hunks for each file.

## P2 - No Dry-Run Mode

There's no way for an agent to preview what would happen before committing to a merge operation. A `--dry-run` flag should:
- List PRs that would be selected
- Show dependency analysis results
- Identify potential conflicts (from file overlap analysis)
- Report without modifying the repository

## P2 - Post-Merge Summary Always Empty

**File:** `src/core/runner/non_interactive.rs:729`

```rust
let summary = SummaryInfo {
    // ...
    post_merge: None,  // Always None, never populated
};
```

The `complete` command runs post-merge tasks (PR tagging, work item updates) but never populates the `post_merge` field in the summary. Agents can't verify which tags and work item updates succeeded or failed.

## P3 - No Retry Configuration for API Calls

The `AzureDevOpsClient` accepts `max_retries` but ignores it. The `RateLimited { retry_after_seconds }` error type exists but there's no auto-retry mechanism. Agents operating in environments with unreliable networks or API rate limits have no way to configure resilience.

## P3 - No Idempotency Guarantees

If `merge continue` crashes mid-operation, the state file may be inconsistent with actual git state. There's no reconciliation mechanism or `--force-continue` to recover. Agents need crash-safe operations.

## P3 - Inconsistent CLI Flags

`merge abort` lacks a `--quiet` flag while all other subcommands have it. Minor but breaks uniform invocation patterns for agents.

## Priority Summary

| Priority | Gap | Type |
|----------|-----|------|
| P0 | `continue` doesn't call `git cherry-pick --continue` | Bug |
| P0 | State file path not in JSON output | Missing feature |
| P1 | No `merge skip` command | Missing command |
| P1 | Error codes always None in JSON | Missing feature |
| P1 | No `merge list` command | Missing command |
| P1 | No `--select-by-pr-id` | Missing feature |
| P2 | No conflict diff content in events | Missing feature |
| P2 | No `--dry-run` mode | Missing feature |
| P2 | Post-merge summary always empty | Bug |
| P3 | No API retry configuration | Missing feature |
| P3 | No crash recovery / idempotency | Missing feature |
| P3 | Inconsistent `--quiet` flag | Polish |
