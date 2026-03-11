#!/usr/bin/env bash
#
# test_scenarios.sh
#
# Integration tests for non-interactive merge (cherry-pick) scenarios.
# Sources setup_test_repo.sh to create a fresh test repository, then runs
# five scenarios that exercise clean picks, conflict detection, resolution,
# abort-and-continue, and mixed sequences.
#
# Usage:
#   ./test_scenarios.sh
#
# Exit code 0 if all scenarios pass, 1 otherwise.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# Counters
# ---------------------------------------------------------------------------
TOTAL=0
PASSED=0
FAILED=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
header() {
    echo ""
    echo "========================================================================"
    echo "  $1"
    echo "========================================================================"
}

assert() {
    local description="$1"
    local result="$2"   # 0 = pass, non-zero = fail

    TOTAL=$((TOTAL + 1))
    if [ "${result}" -eq 0 ]; then
        PASSED=$((PASSED + 1))
        echo "  [PASS] ${description}"
    else
        FAILED=$((FAILED + 1))
        echo "  [FAIL] ${description}"
    fi
}

# Reset `main` in the test repo back to MAIN_CONFLICT so every scenario
# starts from the same state. Also ensure no cherry-pick is in progress.
reset_main() {
    git -C "${TEST_REPO_DIR}" cherry-pick --abort 2>/dev/null || true
    git -C "${TEST_REPO_DIR}" checkout main >/dev/null 2>&1
    git -C "${TEST_REPO_DIR}" reset --hard "${MAIN_CONFLICT}" >/dev/null 2>&1
}

# ---------------------------------------------------------------------------
# Set up the test repository
# ---------------------------------------------------------------------------
echo "Setting up test repository..."
# shellcheck source=setup_test_repo.sh
source "${SCRIPT_DIR}/setup_test_repo.sh"
echo ""

# Save the starting point so we can reset between scenarios.
STARTING_POINT="${MAIN_CONFLICT}"

# ============================= Scenario 1 ==================================
header "Scenario 1: Happy path - all clean cherry-picks"

reset_main

# Cherry-pick Commit A (PR #101)
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_A}" --no-edit >/dev/null 2>&1
assert "Cherry-pick commit A (PR #101) succeeds" $?

# Verify feature_a.rs exists
test -f "${TEST_REPO_DIR}/src/feature_a.rs"
assert "src/feature_a.rs exists after picking A" $?

# Cherry-pick Commit B (PR #102)
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_B}" --no-edit >/dev/null 2>&1
assert "Cherry-pick commit B (PR #102) succeeds" $?

# Verify feature_b.rs exists
test -f "${TEST_REPO_DIR}/src/feature_b.rs"
assert "src/feature_b.rs exists after picking B" $?

# Verify we have exactly 2 new commits on main (initial + conflict-setup + A + B = 4 total)
COMMIT_COUNT="$(git -C "${TEST_REPO_DIR}" rev-list --count HEAD)"
test "${COMMIT_COUNT}" -eq 4
assert "main has 4 total commits (initial + conflict-setup + A + B)" $?

# ============================= Scenario 2 ==================================
header "Scenario 2: Conflict detection"

reset_main

# Cherry-pick Commit C (PR #103) -- expected to fail with conflict
set +e
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_C}" --no-edit >/dev/null 2>&1
PICK_EXIT=$?
set -e

test "${PICK_EXIT}" -ne 0
assert "Cherry-pick commit C exits with non-zero (conflict)" $?

# Verify git detects unmerged files
UNMERGED="$(git -C "${TEST_REPO_DIR}" ls-files -u)"
test -n "${UNMERGED}"
assert "git ls-files -u shows unmerged files" $?

# Verify src/main.rs is specifically the conflicting file
echo "${UNMERGED}" | grep -q "src/main.rs"
assert "src/main.rs is listed as unmerged" $?

# Clean up the in-progress cherry-pick
git -C "${TEST_REPO_DIR}" cherry-pick --abort >/dev/null 2>&1

# ============================= Scenario 3 ==================================
header "Scenario 3: Conflict resolution and continue"

reset_main

# Cherry-pick Commit C (PR #103) -- expected to conflict
set +e
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_C}" --no-edit >/dev/null 2>&1
set -e

# Resolve conflict by accepting "theirs" (the cherry-picked version)
git -C "${TEST_REPO_DIR}" checkout --theirs src/main.rs >/dev/null 2>&1
assert "Resolve conflict by accepting theirs" $?

# Stage the resolved file
git -C "${TEST_REPO_DIR}" add src/main.rs
assert "Stage resolved src/main.rs" $?

# Continue the cherry-pick
git -C "${TEST_REPO_DIR}" cherry-pick --continue --no-edit >/dev/null 2>&1
assert "git cherry-pick --continue succeeds" $?

# Verify the commit was created (3 commits: initial + conflict-setup + resolved C)
COMMIT_COUNT="$(git -C "${TEST_REPO_DIR}" rev-list --count HEAD)"
test "${COMMIT_COUNT}" -eq 3
assert "main has 3 total commits after resolving C" $?

# Verify the content matches the dev-branch version of main.rs
grep -q "Feature C initialised" "${TEST_REPO_DIR}/src/main.rs"
assert "src/main.rs contains 'Feature C initialised' from dev" $?

# ============================= Scenario 4 ==================================
header "Scenario 4: Skip conflicting and continue"

reset_main

# Cherry-pick Commit C (PR #103) -- expected to conflict
set +e
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_C}" --no-edit >/dev/null 2>&1
set -e

# Abort the cherry-pick
git -C "${TEST_REPO_DIR}" cherry-pick --abort >/dev/null 2>&1
assert "Abort cherry-pick of conflicting commit C" $?

# Verify we are back to the clean state
UNMERGED_AFTER_ABORT="$(git -C "${TEST_REPO_DIR}" ls-files -u)"
test -z "${UNMERGED_AFTER_ABORT}"
assert "No unmerged files after abort" $?

# Cherry-pick Commit D (PR #104) -- should succeed cleanly
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_D}" --no-edit >/dev/null 2>&1
assert "Cherry-pick commit D (PR #104) succeeds after aborting C" $?

# Verify feature_d.rs exists
test -f "${TEST_REPO_DIR}/src/feature_d.rs"
assert "src/feature_d.rs exists after picking D" $?

# Verify no feature_c artifacts leaked in
! test -f "${TEST_REPO_DIR}/src/feature_c.rs"
assert "No feature_c.rs file present (C was aborted)" $?

# ============================= Scenario 5 ==================================
header "Scenario 5: Multiple operations sequence"

reset_main

# Step 1: Cherry-pick A (success)
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_A}" --no-edit >/dev/null 2>&1
assert "Step 1: Cherry-pick A succeeds" $?

# Step 2: Cherry-pick C (conflict)
set +e
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_C}" --no-edit >/dev/null 2>&1
PICK_C_EXIT=$?
set -e

test "${PICK_C_EXIT}" -ne 0
assert "Step 2: Cherry-pick C conflicts as expected" $?

# Step 3: Abort C
git -C "${TEST_REPO_DIR}" cherry-pick --abort >/dev/null 2>&1
assert "Step 3: Abort cherry-pick of C" $?

# Step 4: Cherry-pick B (success)
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_B}" --no-edit >/dev/null 2>&1
assert "Step 4: Cherry-pick B succeeds" $?

# Step 5: Cherry-pick D (success)
git -C "${TEST_REPO_DIR}" cherry-pick "${COMMIT_D}" --no-edit >/dev/null 2>&1
assert "Step 5: Cherry-pick D succeeds" $?

# Verify final state
test -f "${TEST_REPO_DIR}/src/feature_a.rs"
assert "Final: feature_a.rs exists" $?

test -f "${TEST_REPO_DIR}/src/feature_b.rs"
assert "Final: feature_b.rs exists" $?

test -f "${TEST_REPO_DIR}/src/feature_d.rs"
assert "Final: feature_d.rs exists" $?

# main.rs should still be the main-branch version (C was aborted)
grep -q "Main startup sequence" "${TEST_REPO_DIR}/src/main.rs"
assert "Final: src/main.rs has main-branch content (C was skipped)" $?

# Total commits: initial + conflict-setup + A + B + D = 5
COMMIT_COUNT="$(git -C "${TEST_REPO_DIR}" rev-list --count HEAD)"
test "${COMMIT_COUNT}" -eq 5
assert "Final: main has 5 total commits" $?

# ---------------------------------------------------------------------------
# Summary and cleanup
# ---------------------------------------------------------------------------
echo ""
echo "========================================================================"
echo "  RESULTS: ${PASSED}/${TOTAL} passed, ${FAILED} failed"
echo "========================================================================"

# Clean up the temporary repository
rm -rf "${TEST_REPO_DIR}"
echo "Cleaned up test repository at ${TEST_REPO_DIR}"

if [ "${FAILED}" -gt 0 ]; then
    exit 1
fi

exit 0
