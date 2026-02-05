# PR Dependency UI Integration - Verification Plan

## Overview

This document describes how to verify each phase of the implementation is working correctly.

---

## Phase 1: Analysis During Data Loading

### Automated Tests

```bash
# Run unit tests for parallel analysis
cargo nextest run dependency_analysis

# Run with coverage
cargo llvm-cov nextest --test dependency_analysis
```

### Manual Verification

1. **Loading Stage Display**
   - Start TUI: `cargo run -- merge`
   - Observe loading stages progress
   - Verify "Analyzing dependencies..." message appears after commit info
   - Verify progress indicator shows PR count

2. **Timing Comparison**
   - With >50 PRs, compare analysis time with/without rayon
   - Expected: ~2-4x speedup on multi-core systems

3. **Graph Correctness**
   - Compare parallel results with sequential (unit test)
   - Verify no missing edges or duplicate entries

### Acceptance Criteria
- [ ] Loading shows analysis stage
- [ ] Analysis completes without error for 100+ PRs
- [ ] Dependency graph accessible in PullRequestSelectionState

---

## Phase 2: Dependency Column

### Automated Tests

```bash
# Run snapshot tests
cargo nextest run pr_selection

# Update snapshots if needed
cargo insta review
```

### Manual Verification

1. **Column Visibility**
   - Start TUI and navigate to PR list
   - Verify "Deps" column header visible
   - Verify column width is appropriate (not cut off)

2. **Count Accuracy**
   - For a known PR with dependencies, verify count matches
   - Cross-reference with `mergers merge status --output json`

3. **Color Coding**
   - PRs with no dependencies: Green or neutral
   - PRs with partial dependencies: Yellow
   - PRs with full dependencies: Red

### Acceptance Criteria
- [ ] Column header shows "Deps"
- [ ] Counts formatted as "P/D"
- [ ] Colors correctly reflect dependency severity

---

## Phase 3: Dependency Dialog

### Automated Tests

```bash
# Run dialog snapshot tests
cargo nextest run dependency_dialog
```

### Manual Verification

1. **Dialog Opens**
   - Press 'd' on highlighted PR
   - Verify dialog appears centered
   - Verify dialog title shows PR ID

2. **Content Accuracy**
   - Dependencies section lists correct PRs
   - Dependents section lists correct PRs
   - File overlaps shown with correct paths

3. **Navigation**
   - ↑/↓ scrolls content
   - Esc closes dialog
   - Dialog doesn't persist state incorrectly

4. **Edge Cases**
   - PR with no dependencies: "No dependencies"
   - PR with many dependencies: Scrollable
   - Very long PR titles: Truncated

### Acceptance Criteria
- [ ] 'd' opens dialog for highlighted PR
- [ ] Dependencies and dependents correctly listed
- [ ] File overlap details visible
- [ ] Keyboard navigation works
- [ ] Esc closes dialog

---

## Phase 4: Unselected Dependency Highlighting

### Automated Tests

```bash
# Run highlighting tests
cargo nextest run unselected_dependency
```

### Manual Verification

1. **No Selection**
   - With no PRs selected, no amber highlights
   - Border should be white/default

2. **Select PR with Dependencies**
   - Select a PR that depends on another PR
   - Verify dependent PR shows amber background
   - Verify border turns yellow

3. **Select Dependent**
   - Select the previously amber PR
   - Verify it turns green (selected)
   - Verify border warning clears if no more missing deps

4. **Multiple Dependencies**
   - Select multiple PRs with overlapping dependencies
   - Verify all unselected deps highlighted
   - Verify count in title is accurate

5. **Transitive Dependencies (if implemented)**
   - A → B → C, select A
   - Verify B highlighted
   - Verify C behavior matches design decision

### Acceptance Criteria
- [ ] Unselected dependencies show amber/orange background
- [ ] Border turns yellow when missing deps exist
- [ ] Title shows count of missing dependencies
- [ ] Highlighting updates on selection change

---

## Phase 5: Status Bar Summary

### Manual Verification

1. **Summary Display**
   - Navigate to PR list
   - Verify status bar shows dependency summary

2. **Real-time Updates**
   - Select/deselect PRs
   - Verify counts update immediately

3. **Format**
   - Verify format: "Selected: N | Missing deps: M | ..."

### Acceptance Criteria
- [ ] Summary visible in status bar
- [ ] Counts accurate
- [ ] Updates on selection change

---

## Integration Verification

### Full Flow Test

```bash
# 1. Start with fresh state
rm -rf ~/.local/state/mergers/

# 2. Run TUI merge mode
cargo run -- merge

# 3. Walk through flow:
#    - Observe loading stages including analysis
#    - View Deps column in PR list
#    - Press 'd' to view dependency dialog
#    - Select PRs and observe highlighting
#    - Verify status bar updates
```

### Cross-Mode Verification

1. **TUI to Non-Interactive**
   - Start TUI, view dependencies
   - Exit and run `mergers merge status`
   - Verify JSON output shows same dependencies

2. **Non-Interactive Consistency**
   - Run `mergers merge -n --version v1.0.0 --select-by-state "Ready"`
   - Verify dependency warnings in output
   - Compare with TUI highlighting

---

## Performance Benchmarks

### Targets

| Scenario | PR Count | Max Analysis Time |
|----------|----------|------------------|
| Small | 10 | < 1s |
| Medium | 50 | < 3s |
| Large | 200 | < 10s |

### Measurement

```bash
# Run with timing
time cargo run -- merge 2>&1 | grep "Analyzing"

# Or add instrumentation in code
let start = std::time::Instant::now();
// ... analysis ...
tracing::info!("Dependency analysis took {:?}", start.elapsed());
```

---

## Regression Tests

After implementation, add these to CI:

```yaml
# .github/workflows/ci.yml additions
- name: Run dependency UI tests
  run: |
    cargo nextest run dependency
    cargo nextest run pr_selection
```

---

## Known Limitations

1. **Shallow Clones**: May fail to fetch commit diffs for old PRs
2. **Large Files**: Analysis may be slow for PRs with many file changes
3. **Terminal Width**: Dialog may not render well on very narrow terminals

---

## Rollback Plan

If issues arise:

1. **Disable Analysis Stage**
   - Skip `AnalyzingDependencies` stage
   - Set `dependency_graph = None`

2. **Hide Column**
   - Remove Deps column from constraints
   - Keep analysis for future use

3. **Disable Highlighting**
   - Set `unselected_deps = HashSet::new()`
   - Remove border color change
