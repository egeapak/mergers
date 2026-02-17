#!/usr/bin/env bash
# find-revert-pairs.sh — detect revert/original commit pairs and output
# --skip-commit flags for git-cliff so both sides of a revert are excluded.
#
# Usage: bash scripts/find-revert-pairs.sh [range]
#   range  optional git revision range (e.g. v0.3.0..HEAD)
#
# Output (stdout): --skip-commit <sha> flags, or nothing if no pairs found.
set -euo pipefail

RANGE="${1:-}"

# ---------------------------------------------------------------------------
# 1. Collect commits: SHA, subject, body  (NUL-delimited for safety)
# ---------------------------------------------------------------------------
log_args=(--format="%H%x00%s%x00%b%x00")
if [[ -n "$RANGE" ]]; then
    log_args+=("$RANGE")
fi

# Associative arrays (bash 4+)
declare -A SUBJECT_BY_SHA   # sha  -> subject
declare -A BODY_BY_SHA      # sha  -> body
declare -A SHA_BY_PR        # "#N" -> sha  (for GitHub-UI revert lookup)
declare -A REVERTS          # revert_sha -> original_sha

# Read log with NUL delimiters.
# git log adds a newline between records, so strip leading/trailing
# whitespace from each field to avoid poisoned keys.
while IFS= read -r -d '' sha && IFS= read -r -d '' subject && IFS= read -r -d '' body; do
    sha="${sha#$'\n'}"
    sha="${sha%$'\n'}"
    body="${body%$'\n'}"

    SUBJECT_BY_SHA["$sha"]="$subject"
    BODY_BY_SHA["$sha"]="$body"

    # Index by PR number if subject ends with (#N)
    if [[ "$subject" =~ \(#([0-9]+)\)$ ]]; then
        pr_num="${BASH_REMATCH[1]}"
        SHA_BY_PR["$pr_num"]="$sha"
    fi
done < <(git log "${log_args[@]}" 2>/dev/null || true)

# ---------------------------------------------------------------------------
# 2. Identify revert -> original pairs
# ---------------------------------------------------------------------------
for sha in "${!SUBJECT_BY_SHA[@]}"; do
    subject="${SUBJECT_BY_SHA[$sha]}"
    body="${BODY_BY_SHA[$sha]}"

    # Only look at commits whose subject starts with 'Revert "'
    [[ "$subject" == Revert\ \"* ]] || continue

    original_sha=""

    # Format A: standard git revert — "This reverts commit <sha>."
    if [[ "$body" =~ This\ reverts\ commit\ ([0-9a-f]{7,40})\. ]]; then
        original_sha="${BASH_REMATCH[1]}"
    fi

    # Format B: GitHub UI revert — "Reverts owner/repo#N"
    if [[ -z "$original_sha" && "$body" =~ Reverts\ [^#]+#([0-9]+) ]]; then
        pr_num="${BASH_REMATCH[1]}"
        original_sha="${SHA_BY_PR[$pr_num]:-}"
    fi

    if [[ -n "$original_sha" ]]; then
        # Resolve short SHA to full if needed
        full_original=$(git rev-parse --verify "$original_sha" 2>/dev/null || true)
        if [[ -n "$full_original" ]]; then
            REVERTS["$sha"]="$full_original"
        fi
    fi
done

# ---------------------------------------------------------------------------
# 3. Cancel double-reverts: if a revert is itself reverted, drop both pairs
#    so the original commit stays in the changelog.
# ---------------------------------------------------------------------------
declare -A CANCELLED
for revert_sha in "${!REVERTS[@]}"; do
    original_sha="${REVERTS[$revert_sha]}"
    # Check if this "original" is itself a revert of something
    if [[ -n "${REVERTS[$original_sha]:-}" ]]; then
        CANCELLED["$revert_sha"]=1
        CANCELLED["$original_sha"]=1
    fi
done

# ---------------------------------------------------------------------------
# 4. Collect unique SHAs to skip
# ---------------------------------------------------------------------------
declare -A SKIP
for revert_sha in "${!REVERTS[@]}"; do
    [[ -n "${CANCELLED[$revert_sha]:-}" ]] && continue
    original_sha="${REVERTS[$revert_sha]}"
    SKIP["$revert_sha"]=1
    SKIP["$original_sha"]=1
done

# ---------------------------------------------------------------------------
# 5. Output --skip-commit flags
# ---------------------------------------------------------------------------
output=""
for sha in "${!SKIP[@]}"; do
    output+="--skip-commit $sha "
done

# Trim trailing space
echo -n "${output% }"
