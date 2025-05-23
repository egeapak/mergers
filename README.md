# Merge Tool

A command-line interface (CLI) tool written in Rust to help streamline the process of merging multiple Azure DevOps pull requests (PRs) into a target branch via cherry-picking. It provides a Text-based User Interface (TUI) for selecting PRs.

## Features

- **Azure DevOps Integration:**
    - Fetches pull requests from a specified Azure DevOps organization, project, and repository.
    - Filters out pull requests that already have a "merged" tag (or similar, based on `api::filter_prs_without_merged_tag`).
    - Retrieves associated work items for each pull request.
- **Interactive Pull Request Selection:**
    - Utilizes a Text-based User Interface (TUI) built with `ratatui` for interactively selecting the pull requests you want to merge.
- **Flexible Git Workflow:**
    - **New Clone:** If no local repository path is provided, it performs a shallow clone of the repository.
    - **Worktree Support:** If a local repository path is provided, it creates a new git worktree, keeping your main working directory clean.
    - **Automated Cherry-Picking:** Cherry-picks the `lastMergeCommit` of the selected pull requests into a new local branch (e.g., `patch/target_branch-version`).
    - **Conflict Handling:** Provides an interactive prompt to pause the process if conflicts occur during cherry-picking, allowing you to resolve them manually before continuing or skipping the problematic commit.
- **Convenient Output:**
    - Displays clickable links to the selected Azure DevOps pull requests and their associated work items in the terminal after successful operation.

## Prerequisites

- **Git:** Must be installed and accessible in your system's PATH. The tool relies on `git` CLI commands for all repository operations.
- **Azure DevOps Personal Access Token (PAT):** You'll need a PAT with appropriate permissions (e.g., Code Read, Work Items Read) to fetch data from Azure DevOps. This token is passed as a command-line argument.
- **Note on PAT Security:** Treat your PAT like a password. Ensure it has the minimum required scopes and consider setting an expiration date. Do not hardcode it directly into scripts or share it publicly.

## Command-Line Arguments

The tool uses the following command-line arguments:

-   `-o, --organization <ORGANIZATION>`: Specifies the Azure DevOps organization.
-   `-p, --project <PROJECT>`: Specifies the Azure DevOps project.
-   `-r, --repository <REPOSITORY>`: Specifies the repository name within the Azure DevOps project.
-   `-t, --pat <PAT>`: Your Azure DevOps Personal Access Token for authentication.
-   `--dev-branch <DEV_BRANCH>`: The development branch from which to fetch pull requests (default: `dev`).
-   `--target-branch <TARGET_BRANCH>`: The target branch into which the changes will be merged (default: `next`). This branch will be the base for the new `patch/...` branch.
-   `--local-repo <LOCAL_REPO>`: Optional. Path to your local git repository. If provided, the tool will create a git worktree within this repository instead of cloning anew. If omitted, the tool will perform a shallow clone of the repository into a temporary directory.

## Usage

1.  **Build the tool** (see [Building](#building) section below).
2.  **Run the tool** from your terminal.

**Example:**

```bash
./target/release/merge-tool \
    -o "MyAzureOrg" \
    -p "MyProject" \
    -r "MyRepo" \
    -t "YOUR_AZURE_DEVOPS_PAT" \
    --dev-branch "develop" \
    --target-branch "release/1.2.0"
```

**Using a local repository:**

If you have a local clone of the repository and prefer to use a git worktree:

```bash
./target/release/merge-tool \
    -o "MyAzureOrg" \
    -p "MyProject" \
    -r "MyRepo" \
    -t "YOUR_AZURE_DEVOPS_PAT" \
    --dev-branch "main" \
    --target-branch "hotfix/1.2.1" \
    --local-repo "/path/to/your/local/clone/MyRepo"
```

Upon running, the tool will:
1. Fetch pull requests from the specified `--dev-branch`.
2. Display a TUI where you can select the PRs to include.
3. Prompt you to enter a version number (this will be used in the new branch name, e.g., `patch/target_branch-version`).
4. Either clone the repository or create a worktree.
5. Cherry-pick the selected PRs into the new branch.
6. If conflicts occur, it will pause and allow you to resolve them.

## TUI Controls

When the list of pull requests is displayed, you can use the following keys to navigate and make selections:

-   **`↑` (Up Arrow)**: Move selection up.
-   **`↓` (Down Arrow)**: Move selection down.
-   **`Space`**: Toggle selection for the currently highlighted pull request. Selected items are marked with `[x]`.
-   **`Enter`**: Confirm your selections and proceed to the next step (entering version number and cherry-picking).
-   **`p`**: Open the currently highlighted pull request in your default web browser.
-   **`w`**: Open the work items associated with the highlighted pull request in your default web browser.
-   **`q`**: Quit the application without proceeding.

## Building

To build the project, you'll need to have Rust and Cargo installed. If you don't have them, please visit [rust-lang.org](https://www.rust-lang.org/tools/install) for installation instructions.

1.  **Clone the repository (if you haven't already):**
    ```bash
    git clone <repository-url>
    cd <repository-directory>
    ```

2.  **Build the project:**
    For a release build (recommended for usage):
    ```bash
    cargo build --release
    ```
    The executable will be located at `target/release/merge-tool`.

    For a debug build:
    ```bash
    cargo build
    ```
    The executable will be located at `target/debug/merge-tool`.
