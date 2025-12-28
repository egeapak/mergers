# PR Dependency UI Integration - Progress Tracker

## Status: Planning Phase

**Last Updated**: 2024-12-28
**Current Phase**: Design Review
**Blocked By**: Design decisions pending user input

---

## Phases Overview

| Phase | Description | Status | Completion |
|-------|-------------|--------|------------|
| 1 | Move Analysis to Data Loading | ⏳ Pending | 0% |
| 2 | Add Dependency Column to PR List | ⏳ Pending | 0% |
| 3 | Dependency Graph Dialog | ⏳ Pending | 0% |
| 4 | Unselected Dependency Highlighting | ⏳ Pending | 0% |
| 5 | Status Bar Summary | ⏳ Pending | 0% |

---

## Phase 1: Move Analysis to Data Loading

### Tasks

- [ ] Add `rayon` to Cargo.toml
- [ ] Add `dependency_graph` field to `MergeApp`
- [ ] Add getter/setter methods for dependency graph
- [ ] Add `AnalyzingDependencies` loading stage
- [ ] Implement `analyze_parallel()` in `DependencyAnalyzer`
- [ ] Integrate analysis into data loading flow
- [ ] Update loading message for analysis stage
- [ ] Add tests for parallel analysis correctness
- [ ] Add snapshot test for loading stage display

### Dependencies
- Design decision: Should analysis run in background thread or block?
- Design decision: Should we show per-file progress?

---

## Phase 2: Add Dependency Column

### Tasks

- [ ] Update column constraints (adjust widths)
- [ ] Add "Deps" column header
- [ ] Implement dependency count cell rendering
- [ ] Add color coding (green/yellow/red)
- [ ] Update header row alignment
- [ ] Add tests for column formatting
- [ ] Add snapshot tests for new column

### Dependencies
- Phase 1 complete (dependency graph available)
- Design decision: Column format P/D vs (P,D) vs P+D

---

## Phase 3: Dependency Dialog

### Tasks

- [ ] Create `DependencyDialogState` struct
- [ ] Add dialog rendering function
- [ ] Implement tree-view for dependencies/dependents
- [ ] Add file overlap details display
- [ ] Handle keyboard navigation (scroll, collapse/expand)
- [ ] Add 'd' keybinding
- [ ] Update help text with new shortcut
- [ ] Add snapshot tests for dialog

### Dependencies
- Phase 1 complete
- Design decision: Show transitive dependencies?
- Design decision: Allow navigating to dependent PR from dialog?

---

## Phase 4: Unselected Dependency Highlighting

### Tasks

- [ ] Implement `compute_unselected_dependencies()`
- [ ] Add highlight color for unselected deps
- [ ] Update row style selection logic
- [ ] Change border color when missing deps
- [ ] Add warning indicator to title
- [ ] Cache computation (recompute on selection change)
- [ ] Add tests for highlight logic
- [ ] Add snapshot tests for highlighting

### Dependencies
- Phase 1 complete
- Design decision: Highlight color choice
- Design decision: Include transitive deps in warning?

---

## Phase 5: Status Bar Summary

### Tasks

- [ ] Add dependency summary to status bar
- [ ] Show selected count with dependency breakdown
- [ ] Update on selection changes
- [ ] Add tests for summary generation

### Dependencies
- Phase 4 complete

---

## Verification Checklist

See `docs/design/pr-dependency-ui-verification.md` for detailed verification steps.

---

## Blockers & Risks

| Issue | Impact | Mitigation |
|-------|--------|------------|
| Large PR count performance | High | Use rayon, add progress indicator |
| Transitive dependency complexity | Medium | Limit depth or make configurable |
| UI space constraints | Medium | Consider collapsible column |

---

## Notes

- Keep dependency analysis cache in MergeApp for re-use
- Consider lazy evaluation for transitive deps
- Dialog should be usable via keyboard only
