#!/usr/bin/env bash
#
# setup_test_repo.sh
#
# Creates a temporary git repository with test commits for testing
# non-interactive merge (cherry-pick) scenarios.
#
# Usage:
#   source setup_test_repo.sh
#
# After sourcing, the following variables are available:
#   TEST_REPO_DIR  - Path to the temporary repository
#   COMMIT_A       - Hash of PR #101 (clean: feature_a.rs)
#   COMMIT_B       - Hash of PR #102 (clean: feature_b.rs)
#   COMMIT_C       - Hash of PR #103 (conflicts with main: main.rs)
#   COMMIT_D       - Hash of PR #104 (clean: feature_d.rs)
#   MAIN_CONFLICT  - Hash of the main-branch commit that conflicts with C
#
# The repo is left checked out on the `main` branch.

set -euo pipefail

# ---------------------------------------------------------------------------
# 1. Create a temporary directory for the test repo
# ---------------------------------------------------------------------------
TEST_REPO_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mergers-test-repo.XXXXXX")"
echo "TEST_REPO_DIR=${TEST_REPO_DIR}"

# ---------------------------------------------------------------------------
# 2. Initialize a git repo with a `main` branch
# ---------------------------------------------------------------------------
git -C "${TEST_REPO_DIR}" init --initial-branch=main >/dev/null 2>&1

# Configure a committer identity so commits succeed in any environment.
git -C "${TEST_REPO_DIR}" config user.email "test@mergers.dev"
git -C "${TEST_REPO_DIR}" config user.name  "Mergers Test"

# Disable commit signing so tests work in environments with signing configured.
git -C "${TEST_REPO_DIR}" config commit.gpgsign false
git -C "${TEST_REPO_DIR}" config tag.gpgsign false

# ---------------------------------------------------------------------------
# 3. Create an initial commit with src/main.rs
# ---------------------------------------------------------------------------
mkdir -p "${TEST_REPO_DIR}/src"
cat > "${TEST_REPO_DIR}/src/main.rs" <<'RUST'
fn main() {
    println!("Hello from main");
}
RUST

git -C "${TEST_REPO_DIR}" add -A
git -C "${TEST_REPO_DIR}" commit -m "Initial commit" >/dev/null

# ---------------------------------------------------------------------------
# 4. Create a `dev` branch from `main`
# ---------------------------------------------------------------------------
git -C "${TEST_REPO_DIR}" checkout -b dev >/dev/null 2>&1

# ---------------------------------------------------------------------------
# 5. On `dev`, create commits that simulate PRs
# ---------------------------------------------------------------------------

# -- Commit A (PR #101): clean change, new file feature_a.rs ---------------
cat > "${TEST_REPO_DIR}/src/feature_a.rs" <<'RUST'
pub fn feature_a() -> &'static str {
    "Feature A implemented"
}
RUST

git -C "${TEST_REPO_DIR}" add src/feature_a.rs
git -C "${TEST_REPO_DIR}" commit -m "PR #101: Add feature A" >/dev/null
COMMIT_A="$(git -C "${TEST_REPO_DIR}" rev-parse HEAD)"
echo "COMMIT_A=${COMMIT_A}"

# -- Commit B (PR #102): clean change, new file feature_b.rs ---------------
cat > "${TEST_REPO_DIR}/src/feature_b.rs" <<'RUST'
pub fn feature_b() -> &'static str {
    "Feature B implemented"
}
RUST

git -C "${TEST_REPO_DIR}" add src/feature_b.rs
git -C "${TEST_REPO_DIR}" commit -m "PR #102: Add feature B" >/dev/null
COMMIT_B="$(git -C "${TEST_REPO_DIR}" rev-parse HEAD)"
echo "COMMIT_B=${COMMIT_B}"

# -- Commit C (PR #103): modify src/main.rs to conflict with main ----------
cat > "${TEST_REPO_DIR}/src/main.rs" <<'RUST'
fn main() {
    println!("Hello from dev branch - PR #103 changes");
    feature_c_init();
}

fn feature_c_init() {
    println!("Feature C initialised");
}
RUST

git -C "${TEST_REPO_DIR}" add src/main.rs
git -C "${TEST_REPO_DIR}" commit -m "PR #103: Modify main.rs (will conflict)" >/dev/null
COMMIT_C="$(git -C "${TEST_REPO_DIR}" rev-parse HEAD)"
echo "COMMIT_C=${COMMIT_C}"

# -- Commit D (PR #104): clean change, new file feature_d.rs ---------------
cat > "${TEST_REPO_DIR}/src/feature_d.rs" <<'RUST'
pub fn feature_d() -> &'static str {
    "Feature D implemented"
}
RUST

git -C "${TEST_REPO_DIR}" add src/feature_d.rs
git -C "${TEST_REPO_DIR}" commit -m "PR #104: Add feature D" >/dev/null
COMMIT_D="$(git -C "${TEST_REPO_DIR}" rev-parse HEAD)"
echo "COMMIT_D=${COMMIT_D}"

# ---------------------------------------------------------------------------
# 6. On `main`, add a commit that creates a conflict with Commit C
# ---------------------------------------------------------------------------
git -C "${TEST_REPO_DIR}" checkout main >/dev/null 2>&1

cat > "${TEST_REPO_DIR}/src/main.rs" <<'RUST'
fn main() {
    println!("Hello from main branch - updated on main");
    run_startup();
}

fn run_startup() {
    println!("Main startup sequence");
}
RUST

git -C "${TEST_REPO_DIR}" add src/main.rs
git -C "${TEST_REPO_DIR}" commit -m "Main: Update main.rs (conflicts with PR #103)" >/dev/null
MAIN_CONFLICT="$(git -C "${TEST_REPO_DIR}" rev-parse HEAD)"
echo "MAIN_CONFLICT=${MAIN_CONFLICT}"

echo ""
echo "Test repository ready at: ${TEST_REPO_DIR}"
echo "Branch 'main' is checked out."
