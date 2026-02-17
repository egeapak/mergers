#!/usr/bin/env bash
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
print_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
print_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Check if version argument is provided
if [ $# -eq 0 ]; then
    print_error "Usage: $0 <version>"
    echo "Example: $0 1.2.3"
    exit 1
fi

VERSION=$1
VERSION_WITH_V="v${VERSION}"

# Validate version format (semantic versioning)
if ! [[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    print_error "Invalid version format. Please use semantic versioning (e.g., 1.2.3)"
    exit 1
fi

print_info "Preparing release ${VERSION_WITH_V}"

# Check if we're in the project root
if [ ! -f "Cargo.toml" ]; then
    print_error "Cargo.toml not found. Please run this script from the project root."
    exit 1
fi

# Check if git-cliff is installed
if ! command -v git-cliff &> /dev/null; then
    print_error "git-cliff is not installed. Please install it first:"
    echo "  cargo install git-cliff"
    exit 1
fi

# Check for uncommitted changes
if [ -n "$(git status --porcelain)" ]; then
    print_warn "You have uncommitted changes. Please commit or stash them first."
    git status --short
    exit 1
fi

# Get current branch
CURRENT_BRANCH=$(git branch --show-current)

# Create release branch if not already on one
RELEASE_BRANCH="release/${VERSION_WITH_V}"
if [ "$CURRENT_BRANCH" != "$RELEASE_BRANCH" ]; then
    print_info "Creating release branch: ${RELEASE_BRANCH}"
    git checkout -b "$RELEASE_BRANCH"
else
    print_info "Already on release branch: ${RELEASE_BRANCH}"
fi

# Update version in Cargo.toml
print_info "Updating version in Cargo.toml to ${VERSION}"
# Use perl for cross-platform compatibility (works on both macOS and Linux)
perl -i -pe 'BEGIN{$found=0} s/^version = "\K[^"]*/'${VERSION}'/ if !$found && /^\[package\]/../^$/ and $found=/^version =/' Cargo.toml

# Update Cargo.lock
print_info "Updating Cargo.lock"
cargo build --quiet 2>&1 | grep -v "Compiling\|Finished" || true

# Detect revert pairs to exclude from changelog
print_info "Detecting revert pairs to exclude from changelog"
SKIP_FLAGS=$(bash scripts/find-revert-pairs.sh)
if [ -n "$SKIP_FLAGS" ]; then
    print_info "Excluding revert pairs: ${SKIP_FLAGS}"
fi

# Generate CHANGELOG.md
print_info "Generating CHANGELOG.md"
# shellcheck disable=SC2086
git-cliff $SKIP_FLAGS --tag "${VERSION_WITH_V}" -o CHANGELOG.md

# Show the changes
print_info "Changes to be committed:"
git diff --stat

echo ""
print_info "Summary of changes:"
echo "  - Version bumped to ${VERSION} in Cargo.toml"
echo "  - Cargo.lock updated"
echo "  - CHANGELOG.md generated"

echo ""
read -p "$(echo -e ${GREEN}[PROMPT]${NC} Do you want to commit these changes? [y/N]: )" -n 1 -r
echo

if [[ $REPLY =~ ^[Yy]$ ]]; then
    # Commit the changes
    git add Cargo.toml Cargo.lock CHANGELOG.md
    git commit -m "chore(release): prepare ${VERSION_WITH_V}"

    print_info "Changes committed successfully!"
    echo ""
    print_info "Next steps:"
    echo "  1. Push the branch: git push origin ${RELEASE_BRANCH}"
    echo "  2. Create a pull request to master"
    echo "  3. After merge, create and push the tag:"
    echo "     git checkout master"
    echo "     git pull"
    echo "     git tag ${VERSION_WITH_V}"
    echo "     git push origin ${VERSION_WITH_V}"
else
    print_warn "Changes not committed. You can review and commit manually."
    echo ""
    echo "To commit manually:"
    echo "  git add Cargo.toml Cargo.lock CHANGELOG.md"
    echo "  git commit -m \"chore(release): prepare ${VERSION_WITH_V}\""
fi
