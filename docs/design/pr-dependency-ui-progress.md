# PR Dependency UI Integration - Progress Tracker

## Status: Phases 1-4 Complete, Starting Phase 5

**Last Updated**: 2025-12-28
**Current Phase**: Implementation Phase 5
**Blocked By**: None

---

## Phases Overview

| Phase | Description | Status | Completion |
|-------|-------------|--------|------------|
| 1 | Move Analysis to Data Loading | ✅ Complete | 100% |
| 2 | Add Dependency Column to PR List | ✅ Complete | 100% |
| 3 | Dependency Graph Dialog | ✅ Complete | 100% |
| 4 | Unselected Dependency Highlighting | ✅ Complete | 100% |
| 5 | Status Bar Summary | ⏳ Pending | 0% |

---

## Phase 1: Move Analysis to Data Loading ✅

### Tasks

- [x] Add `rayon` to Cargo.toml
- [x] Add `dependency_graph` field to `MergeApp`
- [x] Add getter/setter methods for dependency graph
- [x] Add `AnalyzingDependencies` loading stage enum variant
- [x] Implement `analyze_parallel()` in `DependencyAnalyzer`
- [x] Integrate analysis into data loading flow (blocking with progress)
- [x] Update loading message: "Analyzing dependencies (N PRs)..."
- [x] Add tests for parallel analysis correctness
- [x] Add snapshot test for loading stage display

### Notes
- Analysis requires `local_repo` config to be set (skipped if not available)
- Parallel analysis uses rayon for O(n^2) pairwise comparison
- Added tests: `test_parallel_analysis_equivalence`, `test_parallel_analysis_many_prs`

### Design Decisions (Finalized)
- Analysis runs blocking with progress indicator
- Rayon is always included (not feature-flagged)

---

## Phase 2: Add Dependency Column ✅

### Tasks

- [x] Update column constraints (adjust widths for 7 columns)
- [x] Add "Deps" column header
- [x] Implement dependency count cell rendering with `P/D` format
- [x] Add color coding: Green (0/0), Yellow (P>0), Red (D>0)
- [x] Update header row alignment
- [x] Add helper functions: `get_dependency_counts()`, `get_deps_style()`, `format_deps_count()`
- [x] Update snapshot tests for new column (14 snapshots updated)

### Design Decisions (Finalized)
- Column format: `P/D` (partial/full counts)

---

## Phase 3: Dependency Dialog ✅

### Tasks

- [x] Add dialog state fields to `PullRequestSelectionState`
- [x] Add dialog rendering function (centered overlay)
- [x] Implement full transitive dependency tree computation (BFS)
- [x] Render tree-view for dependencies/dependents
- [x] Color direct dependencies in Cyan
- [x] Color transitive dependencies in Gray
- [x] Handle keyboard navigation (scroll with ↑/↓, close with Esc/d/q)
- [x] Add 'd' keybinding
- [x] Update help text with new shortcut

### Notes
- Added `DependencyEntry` type alias for cleaner function signatures
- Dialog shows dependencies (PRs this PR depends on) and dependents (PRs that depend on this PR)
- BFS traversal for transitive dependency computation
- Direct deps shown in Cyan, transitive deps in DarkGray
- Graceful handling when dependency graph is not available

### Design Decisions (Finalized)
- Show full transitive tree with color differentiation
- Direct deps: Cyan, Transitive deps: Gray
- Dialog is view-only (navigation to PRs deferred)

---

## Phase 4: Unselected Dependency Highlighting ✅

### Tasks

- [x] Implement `compute_unselected_dependencies()`
- [x] Add Orange/Amber highlight `Rgb(80, 40, 0)` for unselected deps
- [x] Update row style selection logic
- [x] Change border color to Yellow when missing deps
- [x] Add warning indicator to title: `"Pull Requests (⚠ N missing deps)"`

### Notes
- Computation runs during render (no caching needed as it's fast)
- Priority order: Selected (green) > Unselected dep (orange/amber) > Search results (blue)
- Only shows PRs that are in the current list (not already merged)

### Design Decisions (Finalized)
- Highlight color: Orange/Amber `Rgb(80, 40, 0)`
- Border: Yellow when missing dependencies
- Title shows warning indicator with count

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
