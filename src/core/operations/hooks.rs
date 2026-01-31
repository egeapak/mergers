//! Post-merge hook execution for merge workflows.
//!
//! This module provides the infrastructure for running user-defined shell commands
//! at various points during the merge workflow. Hooks are configurable per-project
//! via the configuration file.
//!
//! # Hook Types
//!
//! - `post_checkout` - Runs after the repository is set up (clone or worktree created)
//! - `pre_cherry_pick` - Runs before starting the cherry-pick process
//! - `post_cherry_pick` - Runs after each successful cherry-pick
//! - `post_merge` - Runs after all cherry-picks complete successfully
//! - `on_conflict` - Runs when a conflict is detected
//! - `post_complete` - Runs after the 'complete' command finishes (tagging, work item updates)
//!
//! # Configuration
//!
//! Hooks are configured in the `[hooks]` section of the config file:
//!
//! ```toml
//! [hooks]
//! post_checkout = ["npm install", "cargo build"]
//! post_cherry_pick = ["cargo fmt", "cargo clippy --fix --allow-dirty"]
//! post_merge = ["cargo test"]
//! on_conflict = ["git status"]
//! post_complete = ["./scripts/notify.sh"]
//! ```
//!
//! # Environment Variables
//!
//! Hooks receive the following environment variables:
//!
//! - `MERGERS_VERSION` - The merge version (e.g., "v1.0.0")
//! - `MERGERS_TARGET_BRANCH` - The target branch name
//! - `MERGERS_DEV_BRANCH` - The development branch name
//! - `MERGERS_REPO_PATH` - Path to the repository
//! - `MERGERS_PR_ID` - The PR ID (for post_cherry_pick and on_conflict hooks)
//! - `MERGERS_COMMIT_ID` - The commit ID (for post_cherry_pick hooks)

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// Hook trigger points in the merge workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookTrigger {
    /// After repository setup (clone/worktree created).
    PostCheckout,
    /// Before starting cherry-pick process.
    PreCherryPick,
    /// After each successful cherry-pick.
    PostCherryPick,
    /// After all cherry-picks complete.
    PostMerge,
    /// When a conflict is detected.
    OnConflict,
    /// After the complete command finishes.
    PostComplete,
}

impl HookTrigger {
    /// Returns the configuration key name for this trigger.
    pub fn config_key(&self) -> &'static str {
        match self {
            HookTrigger::PostCheckout => "post_checkout",
            HookTrigger::PreCherryPick => "pre_cherry_pick",
            HookTrigger::PostCherryPick => "post_cherry_pick",
            HookTrigger::PostMerge => "post_merge",
            HookTrigger::OnConflict => "on_conflict",
            HookTrigger::PostComplete => "post_complete",
        }
    }

    /// Returns a human-readable description of this trigger.
    pub fn description(&self) -> &'static str {
        match self {
            HookTrigger::PostCheckout => "post-checkout",
            HookTrigger::PreCherryPick => "pre-cherry-pick",
            HookTrigger::PostCherryPick => "post-cherry-pick",
            HookTrigger::PostMerge => "post-merge",
            HookTrigger::OnConflict => "on-conflict",
            HookTrigger::PostComplete => "post-complete",
        }
    }
}

/// Configuration for hooks.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HooksConfig {
    /// Commands to run after repository checkout/setup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_checkout: Vec<String>,

    /// Commands to run before starting cherry-picks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_cherry_pick: Vec<String>,

    /// Commands to run after each successful cherry-pick.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_cherry_pick: Vec<String>,

    /// Commands to run after all cherry-picks complete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_merge: Vec<String>,

    /// Commands to run when a conflict is detected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_conflict: Vec<String>,

    /// Commands to run after the complete command finishes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_complete: Vec<String>,
}

impl HooksConfig {
    /// Creates a new empty hooks configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the commands for a given trigger.
    pub fn commands_for(&self, trigger: HookTrigger) -> &[String] {
        match trigger {
            HookTrigger::PostCheckout => &self.post_checkout,
            HookTrigger::PreCherryPick => &self.pre_cherry_pick,
            HookTrigger::PostCherryPick => &self.post_cherry_pick,
            HookTrigger::PostMerge => &self.post_merge,
            HookTrigger::OnConflict => &self.on_conflict,
            HookTrigger::PostComplete => &self.post_complete,
        }
    }

    /// Returns true if any hooks are configured.
    pub fn has_hooks(&self) -> bool {
        !self.post_checkout.is_empty()
            || !self.pre_cherry_pick.is_empty()
            || !self.post_cherry_pick.is_empty()
            || !self.post_merge.is_empty()
            || !self.on_conflict.is_empty()
            || !self.post_complete.is_empty()
    }

    /// Returns true if hooks are configured for the given trigger.
    pub fn has_hooks_for(&self, trigger: HookTrigger) -> bool {
        !self.commands_for(trigger).is_empty()
    }

    /// Merges another hooks config into this one, with other taking precedence.
    ///
    /// If the other config has any hooks for a trigger, they replace this config's hooks.
    pub fn merge(self, other: Self) -> Self {
        Self {
            post_checkout: if other.post_checkout.is_empty() {
                self.post_checkout
            } else {
                other.post_checkout
            },
            pre_cherry_pick: if other.pre_cherry_pick.is_empty() {
                self.pre_cherry_pick
            } else {
                other.pre_cherry_pick
            },
            post_cherry_pick: if other.post_cherry_pick.is_empty() {
                self.post_cherry_pick
            } else {
                other.post_cherry_pick
            },
            post_merge: if other.post_merge.is_empty() {
                self.post_merge
            } else {
                other.post_merge
            },
            on_conflict: if other.on_conflict.is_empty() {
                self.on_conflict
            } else {
                other.on_conflict
            },
            post_complete: if other.post_complete.is_empty() {
                self.post_complete
            } else {
                other.post_complete
            },
        }
    }
}

/// Context passed to hooks for environment variable population.
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    /// The merge version (e.g., "v1.0.0").
    pub version: Option<String>,
    /// The target branch name.
    pub target_branch: Option<String>,
    /// The development branch name.
    pub dev_branch: Option<String>,
    /// Path to the repository.
    pub repo_path: Option<String>,
    /// The current PR ID (for cherry-pick related hooks).
    pub pr_id: Option<i32>,
    /// The current commit ID (for cherry-pick related hooks).
    pub commit_id: Option<String>,
}

impl HookContext {
    /// Creates a new empty hook context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Sets the target branch.
    pub fn with_target_branch(mut self, branch: impl Into<String>) -> Self {
        self.target_branch = Some(branch.into());
        self
    }

    /// Sets the dev branch.
    pub fn with_dev_branch(mut self, branch: impl Into<String>) -> Self {
        self.dev_branch = Some(branch.into());
        self
    }

    /// Sets the repository path.
    pub fn with_repo_path(mut self, path: impl Into<String>) -> Self {
        self.repo_path = Some(path.into());
        self
    }

    /// Sets the PR ID.
    pub fn with_pr_id(mut self, pr_id: i32) -> Self {
        self.pr_id = Some(pr_id);
        self
    }

    /// Sets the commit ID.
    pub fn with_commit_id(mut self, commit_id: impl Into<String>) -> Self {
        self.commit_id = Some(commit_id.into());
        self
    }

    /// Converts the context to environment variables.
    pub fn to_env_vars(&self) -> HashMap<String, String> {
        let mut vars = HashMap::new();

        if let Some(ref version) = self.version {
            vars.insert("MERGERS_VERSION".to_string(), version.clone());
        }
        if let Some(ref branch) = self.target_branch {
            vars.insert("MERGERS_TARGET_BRANCH".to_string(), branch.clone());
        }
        if let Some(ref branch) = self.dev_branch {
            vars.insert("MERGERS_DEV_BRANCH".to_string(), branch.clone());
        }
        if let Some(ref path) = self.repo_path {
            vars.insert("MERGERS_REPO_PATH".to_string(), path.clone());
        }
        if let Some(pr_id) = self.pr_id {
            vars.insert("MERGERS_PR_ID".to_string(), pr_id.to_string());
        }
        if let Some(ref commit_id) = self.commit_id {
            vars.insert("MERGERS_COMMIT_ID".to_string(), commit_id.clone());
        }

        vars
    }
}

/// Result of a single hook command execution.
#[derive(Debug, Clone)]
pub struct HookCommandResult {
    /// The command that was executed.
    pub command: String,
    /// Whether the command succeeded (exit code 0).
    pub success: bool,
    /// The exit code of the command.
    pub exit_code: Option<i32>,
    /// Standard output from the command.
    pub stdout: String,
    /// Standard error from the command.
    pub stderr: String,
}

impl HookCommandResult {
    /// Returns true if the command succeeded.
    pub fn is_success(&self) -> bool {
        self.success
    }
}

/// Result of running all hooks for a trigger.
#[derive(Debug, Clone)]
pub struct HookResult {
    /// The trigger that was executed.
    pub trigger: HookTrigger,
    /// Results for each command.
    pub command_results: Vec<HookCommandResult>,
    /// Whether all commands succeeded.
    pub all_succeeded: bool,
}

impl HookResult {
    /// Returns the first failed command result, if any.
    pub fn first_failure(&self) -> Option<&HookCommandResult> {
        self.command_results.iter().find(|r| !r.success)
    }
}

/// Progress update for hook execution.
#[derive(Debug, Clone)]
pub enum HookProgress {
    /// Starting to run hooks for a trigger.
    Starting {
        /// The trigger being executed.
        trigger: HookTrigger,
        /// Total number of commands to run.
        command_count: usize,
    },
    /// A command is starting.
    CommandStarting {
        /// The trigger being executed.
        trigger: HookTrigger,
        /// The command being run.
        command: String,
        /// Zero-based index of the command.
        index: usize,
    },
    /// A command completed.
    CommandCompleted {
        /// The trigger being executed.
        trigger: HookTrigger,
        /// The command that ran.
        command: String,
        /// Whether it succeeded.
        success: bool,
        /// Zero-based index of the command.
        index: usize,
    },
    /// All hooks for a trigger completed.
    Completed {
        /// The trigger that was executed.
        trigger: HookTrigger,
        /// Whether all commands succeeded.
        all_succeeded: bool,
    },
}

/// Executor for running hooks.
pub struct HookExecutor {
    config: HooksConfig,
}

impl HookExecutor {
    /// Creates a new hook executor with the given configuration.
    pub fn new(config: HooksConfig) -> Self {
        Self { config }
    }

    /// Returns the hooks configuration.
    pub fn config(&self) -> &HooksConfig {
        &self.config
    }

    /// Returns true if hooks are configured for the given trigger.
    pub fn has_hooks_for(&self, trigger: HookTrigger) -> bool {
        self.config.has_hooks_for(trigger)
    }

    /// Runs all hooks for a given trigger.
    ///
    /// # Arguments
    ///
    /// * `trigger` - The hook trigger point
    /// * `working_dir` - The working directory for commands
    /// * `context` - Context for environment variables
    /// * `progress_callback` - Optional callback for progress updates
    ///
    /// # Returns
    ///
    /// A `HookResult` containing the outcome of all commands.
    pub fn run_hooks<F>(
        &self,
        trigger: HookTrigger,
        working_dir: &Path,
        context: &HookContext,
        mut progress_callback: Option<F>,
    ) -> HookResult
    where
        F: FnMut(HookProgress),
    {
        let commands = self.config.commands_for(trigger);

        if commands.is_empty() {
            return HookResult {
                trigger,
                command_results: vec![],
                all_succeeded: true,
            };
        }

        if let Some(ref mut callback) = progress_callback {
            callback(HookProgress::Starting {
                trigger,
                command_count: commands.len(),
            });
        }

        let env_vars = context.to_env_vars();
        let mut command_results = Vec::with_capacity(commands.len());
        let mut all_succeeded = true;

        for (index, command) in commands.iter().enumerate() {
            if let Some(ref mut callback) = progress_callback {
                callback(HookProgress::CommandStarting {
                    trigger,
                    command: command.clone(),
                    index,
                });
            }

            let result = run_shell_command(command, working_dir, &env_vars);
            let success = result.success;

            if let Some(ref mut callback) = progress_callback {
                callback(HookProgress::CommandCompleted {
                    trigger,
                    command: command.clone(),
                    success,
                    index,
                });
            }

            if !success {
                all_succeeded = false;
            }

            command_results.push(result);

            // Stop on first failure
            if !success {
                break;
            }
        }

        if let Some(ref mut callback) = progress_callback {
            callback(HookProgress::Completed {
                trigger,
                all_succeeded,
            });
        }

        HookResult {
            trigger,
            command_results,
            all_succeeded,
        }
    }

    /// Runs hooks for a trigger without progress callbacks.
    pub fn run_hooks_simple(
        &self,
        trigger: HookTrigger,
        working_dir: &Path,
        context: &HookContext,
    ) -> HookResult {
        self.run_hooks::<fn(HookProgress)>(trigger, working_dir, context, None)
    }
}

/// Runs a single shell command.
fn run_shell_command(
    command: &str,
    working_dir: &Path,
    env_vars: &HashMap<String, String>,
) -> HookCommandResult {
    // Use sh -c on Unix, cmd /C on Windows
    #[cfg(unix)]
    let (shell, shell_arg) = ("sh", "-c");

    #[cfg(windows)]
    let (shell, shell_arg) = ("cmd", "/C");

    let result = Command::new(shell)
        .arg(shell_arg)
        .arg(command)
        .current_dir(working_dir)
        .envs(env_vars)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match result {
        Ok(output) => {
            let exit_code = output.status.code();
            HookCommandResult {
                command: command.to_string(),
                success: output.status.success(),
                exit_code,
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            }
        }
        Err(e) => HookCommandResult {
            command: command.to_string(),
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: format!("Failed to execute command: {}", e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// # Hook Trigger Config Keys
    ///
    /// Verifies that each trigger has the correct config key.
    ///
    /// ## Test Scenario
    /// - Checks config_key for each HookTrigger variant
    ///
    /// ## Expected Outcome
    /// - Keys match expected TOML field names
    #[test]
    fn test_hook_trigger_config_keys() {
        assert_eq!(HookTrigger::PostCheckout.config_key(), "post_checkout");
        assert_eq!(HookTrigger::PreCherryPick.config_key(), "pre_cherry_pick");
        assert_eq!(HookTrigger::PostCherryPick.config_key(), "post_cherry_pick");
        assert_eq!(HookTrigger::PostMerge.config_key(), "post_merge");
        assert_eq!(HookTrigger::OnConflict.config_key(), "on_conflict");
        assert_eq!(HookTrigger::PostComplete.config_key(), "post_complete");
    }

    /// # Hook Trigger Descriptions
    ///
    /// Verifies that each trigger has a human-readable description.
    ///
    /// ## Test Scenario
    /// - Checks description for each HookTrigger variant
    ///
    /// ## Expected Outcome
    /// - Descriptions are non-empty and readable
    #[test]
    fn test_hook_trigger_descriptions() {
        assert!(!HookTrigger::PostCheckout.description().is_empty());
        assert!(!HookTrigger::PreCherryPick.description().is_empty());
        assert!(!HookTrigger::PostCherryPick.description().is_empty());
        assert!(!HookTrigger::PostMerge.description().is_empty());
        assert!(!HookTrigger::OnConflict.description().is_empty());
        assert!(!HookTrigger::PostComplete.description().is_empty());
    }

    /// # Hooks Config Default
    ///
    /// Verifies that default HooksConfig has no hooks.
    ///
    /// ## Test Scenario
    /// - Creates default HooksConfig
    ///
    /// ## Expected Outcome
    /// - All hook lists are empty
    #[test]
    fn test_hooks_config_default() {
        let config = HooksConfig::default();
        assert!(config.post_checkout.is_empty());
        assert!(config.pre_cherry_pick.is_empty());
        assert!(config.post_cherry_pick.is_empty());
        assert!(config.post_merge.is_empty());
        assert!(config.on_conflict.is_empty());
        assert!(config.post_complete.is_empty());
        assert!(!config.has_hooks());
    }

    /// # Hooks Config Has Hooks
    ///
    /// Verifies has_hooks returns true when any hooks are configured.
    ///
    /// ## Test Scenario
    /// - Creates config with one hook
    ///
    /// ## Expected Outcome
    /// - has_hooks returns true
    #[test]
    fn test_hooks_config_has_hooks() {
        let config = HooksConfig::default();
        assert!(!config.has_hooks());

        let config_with_hooks = HooksConfig {
            post_merge: vec!["echo test".to_string()],
            ..Default::default()
        };
        assert!(config_with_hooks.has_hooks());
    }

    /// # Hooks Config Has Hooks For Trigger
    ///
    /// Verifies has_hooks_for checks specific trigger.
    ///
    /// ## Test Scenario
    /// - Creates config with hooks for one trigger
    ///
    /// ## Expected Outcome
    /// - has_hooks_for returns true only for that trigger
    #[test]
    fn test_hooks_config_has_hooks_for() {
        let config = HooksConfig {
            post_merge: vec!["echo test".to_string()],
            ..Default::default()
        };

        assert!(config.has_hooks_for(HookTrigger::PostMerge));
        assert!(!config.has_hooks_for(HookTrigger::PostCheckout));
        assert!(!config.has_hooks_for(HookTrigger::OnConflict));
    }

    /// # Hooks Config Commands For Trigger
    ///
    /// Verifies commands_for returns correct commands.
    ///
    /// ## Test Scenario
    /// - Creates config with multiple hooks
    ///
    /// ## Expected Outcome
    /// - commands_for returns correct slice for each trigger
    #[test]
    fn test_hooks_config_commands_for() {
        let config = HooksConfig {
            post_checkout: vec!["echo checkout".to_string()],
            pre_cherry_pick: vec!["echo pre".to_string()],
            post_cherry_pick: vec!["echo post".to_string()],
            post_merge: vec!["echo merge1".to_string(), "echo merge2".to_string()],
            on_conflict: vec!["echo conflict".to_string()],
            post_complete: vec!["echo complete".to_string()],
        };

        assert_eq!(config.commands_for(HookTrigger::PostCheckout).len(), 1);
        assert_eq!(config.commands_for(HookTrigger::PostMerge).len(), 2);
    }

    /// # Hooks Config Merge
    ///
    /// Verifies that merging configs works correctly.
    ///
    /// ## Test Scenario
    /// - Creates two configs with overlapping hooks
    /// - Merges them
    ///
    /// ## Expected Outcome
    /// - Other config's hooks take precedence when present
    #[test]
    fn test_hooks_config_merge() {
        let base = HooksConfig {
            post_merge: vec!["base cmd".to_string()],
            on_conflict: vec!["base conflict".to_string()],
            ..Default::default()
        };

        let other = HooksConfig {
            post_merge: vec!["other cmd".to_string()],
            post_complete: vec!["other complete".to_string()],
            ..Default::default()
        };

        let merged = base.merge(other);

        // Other takes precedence
        assert_eq!(merged.post_merge, vec!["other cmd".to_string()]);
        // Base kept when other is empty
        assert_eq!(merged.on_conflict, vec!["base conflict".to_string()]);
        // Other added
        assert_eq!(merged.post_complete, vec!["other complete".to_string()]);
    }

    /// # Hook Context Builder
    ///
    /// Verifies the builder pattern for HookContext.
    ///
    /// ## Test Scenario
    /// - Builds a context with all fields
    ///
    /// ## Expected Outcome
    /// - All fields are set correctly
    #[test]
    fn test_hook_context_builder() {
        let context = HookContext::new()
            .with_version("v1.0.0")
            .with_target_branch("main")
            .with_dev_branch("dev")
            .with_repo_path("/path/to/repo")
            .with_pr_id(42)
            .with_commit_id("abc123");

        assert_eq!(context.version, Some("v1.0.0".to_string()));
        assert_eq!(context.target_branch, Some("main".to_string()));
        assert_eq!(context.dev_branch, Some("dev".to_string()));
        assert_eq!(context.repo_path, Some("/path/to/repo".to_string()));
        assert_eq!(context.pr_id, Some(42));
        assert_eq!(context.commit_id, Some("abc123".to_string()));
    }

    /// # Hook Context To Env Vars
    ///
    /// Verifies that context is correctly converted to environment variables.
    ///
    /// ## Test Scenario
    /// - Creates a context with various fields
    /// - Converts to env vars
    ///
    /// ## Expected Outcome
    /// - All set fields become environment variables
    #[test]
    fn test_hook_context_to_env_vars() {
        let context = HookContext::new()
            .with_version("v1.0.0")
            .with_target_branch("main")
            .with_pr_id(42);

        let vars = context.to_env_vars();

        assert_eq!(vars.get("MERGERS_VERSION"), Some(&"v1.0.0".to_string()));
        assert_eq!(vars.get("MERGERS_TARGET_BRANCH"), Some(&"main".to_string()));
        assert_eq!(vars.get("MERGERS_PR_ID"), Some(&"42".to_string()));
        assert!(!vars.contains_key("MERGERS_DEV_BRANCH"));
    }

    /// # Hook Executor Run Empty Hooks
    ///
    /// Verifies that running hooks with no commands succeeds.
    ///
    /// ## Test Scenario
    /// - Creates executor with empty config
    /// - Runs hooks for a trigger
    ///
    /// ## Expected Outcome
    /// - Returns success with empty results
    #[test]
    fn test_hook_executor_empty_hooks() {
        let executor = HookExecutor::new(HooksConfig::default());
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new();

        let result = executor.run_hooks_simple(HookTrigger::PostMerge, temp_dir.path(), &context);

        assert!(result.all_succeeded);
        assert!(result.command_results.is_empty());
    }

    /// # Hook Executor Run Simple Command
    ///
    /// Verifies that a simple command can be executed.
    ///
    /// ## Test Scenario
    /// - Creates executor with a simple echo command
    /// - Runs the hook
    ///
    /// ## Expected Outcome
    /// - Command succeeds with expected output
    #[test]
    fn test_hook_executor_run_simple_command() {
        let config = HooksConfig {
            post_merge: vec!["echo hello".to_string()],
            ..Default::default()
        };
        let executor = HookExecutor::new(config);
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new();

        let result = executor.run_hooks_simple(HookTrigger::PostMerge, temp_dir.path(), &context);

        assert!(result.all_succeeded);
        assert_eq!(result.command_results.len(), 1);
        assert!(result.command_results[0].success);
        assert!(result.command_results[0].stdout.contains("hello"));
    }

    /// # Hook Executor Run Multiple Commands
    ///
    /// Verifies that multiple commands are executed in order.
    ///
    /// ## Test Scenario
    /// - Creates executor with multiple commands
    /// - Runs the hooks
    ///
    /// ## Expected Outcome
    /// - All commands execute in order
    #[test]
    fn test_hook_executor_run_multiple_commands() {
        let config = HooksConfig {
            post_merge: vec!["echo first".to_string(), "echo second".to_string()],
            ..Default::default()
        };
        let executor = HookExecutor::new(config);
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new();

        let result = executor.run_hooks_simple(HookTrigger::PostMerge, temp_dir.path(), &context);

        assert!(result.all_succeeded);
        assert_eq!(result.command_results.len(), 2);
        assert!(result.command_results[0].stdout.contains("first"));
        assert!(result.command_results[1].stdout.contains("second"));
    }

    /// # Hook Executor Stop On Failure
    ///
    /// Verifies that execution stops on first failure.
    ///
    /// ## Test Scenario
    /// - Creates executor with a failing command followed by another
    /// - Runs the hooks
    ///
    /// ## Expected Outcome
    /// - Stops after first failure, second command not run
    #[test]
    fn test_hook_executor_stop_on_failure() {
        let config = HooksConfig {
            post_merge: vec![
                "exit 1".to_string(), // This fails
                "echo should_not_run".to_string(),
            ],
            ..Default::default()
        };
        let executor = HookExecutor::new(config);
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new();

        let result = executor.run_hooks_simple(HookTrigger::PostMerge, temp_dir.path(), &context);

        assert!(!result.all_succeeded);
        assert_eq!(result.command_results.len(), 1);
        assert!(!result.command_results[0].success);
    }

    /// # Hook Executor Environment Variables
    ///
    /// Verifies that environment variables are passed to commands.
    ///
    /// ## Test Scenario
    /// - Creates context with version
    /// - Runs command that echoes the env var
    ///
    /// ## Expected Outcome
    /// - Command receives and can use the env var
    #[test]
    fn test_hook_executor_env_vars() {
        let config = HooksConfig {
            post_merge: vec!["echo $MERGERS_VERSION".to_string()],
            ..Default::default()
        };
        let executor = HookExecutor::new(config);
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new().with_version("v1.2.3");

        let result = executor.run_hooks_simple(HookTrigger::PostMerge, temp_dir.path(), &context);

        assert!(result.all_succeeded);
        assert!(result.command_results[0].stdout.contains("v1.2.3"));
    }

    /// # Hook Result First Failure
    ///
    /// Verifies first_failure returns the first failed command.
    ///
    /// ## Test Scenario
    /// - Creates result with mixed success/failure
    ///
    /// ## Expected Outcome
    /// - first_failure returns the correct failure
    #[test]
    fn test_hook_result_first_failure() {
        let result = HookResult {
            trigger: HookTrigger::PostMerge,
            command_results: vec![
                HookCommandResult {
                    command: "echo ok".to_string(),
                    success: true,
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                },
                HookCommandResult {
                    command: "exit 1".to_string(),
                    success: false,
                    exit_code: Some(1),
                    stdout: String::new(),
                    stderr: "error".to_string(),
                },
            ],
            all_succeeded: false,
        };

        let failure = result.first_failure();
        assert!(failure.is_some());
        assert_eq!(failure.unwrap().command, "exit 1");
    }

    /// # Hooks Config Serialization
    ///
    /// Verifies that HooksConfig serializes/deserializes correctly.
    ///
    /// ## Test Scenario
    /// - Creates a config with hooks
    /// - Serializes to TOML and deserializes back
    ///
    /// ## Expected Outcome
    /// - Deserialized config matches original
    #[test]
    fn test_hooks_config_serialization() {
        let config = HooksConfig {
            post_merge: vec!["cargo test".to_string()],
            on_conflict: vec!["git status".to_string()],
            ..Default::default()
        };

        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: HooksConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(config, deserialized);
    }

    /// # Hooks Config TOML Parsing
    ///
    /// Verifies that HooksConfig parses from TOML correctly.
    ///
    /// ## Test Scenario
    /// - Parses a TOML string with hooks
    ///
    /// ## Expected Outcome
    /// - Config has correct values
    #[test]
    fn test_hooks_config_toml_parsing() {
        let toml_str = r#"
post_checkout = ["npm install"]
post_merge = ["cargo test", "cargo build"]
on_conflict = ["git status"]
"#;

        let config: HooksConfig = toml::from_str(toml_str).unwrap();

        assert_eq!(config.post_checkout, vec!["npm install".to_string()]);
        assert_eq!(
            config.post_merge,
            vec!["cargo test".to_string(), "cargo build".to_string()]
        );
        assert_eq!(config.on_conflict, vec!["git status".to_string()]);
        assert!(config.pre_cherry_pick.is_empty());
        assert!(config.post_cherry_pick.is_empty());
        assert!(config.post_complete.is_empty());
    }

    /// # Hook Progress Variants
    ///
    /// Verifies that all progress variants can be created.
    ///
    /// ## Test Scenario
    /// - Creates each progress variant
    ///
    /// ## Expected Outcome
    /// - All variants construct successfully
    #[test]
    fn test_hook_progress_variants() {
        let _p1 = HookProgress::Starting {
            trigger: HookTrigger::PostMerge,
            command_count: 2,
        };
        let _p2 = HookProgress::CommandStarting {
            trigger: HookTrigger::PostMerge,
            command: "echo test".to_string(),
            index: 0,
        };
        let _p3 = HookProgress::CommandCompleted {
            trigger: HookTrigger::PostMerge,
            command: "echo test".to_string(),
            success: true,
            index: 0,
        };
        let _p4 = HookProgress::Completed {
            trigger: HookTrigger::PostMerge,
            all_succeeded: true,
        };
    }

    /// # Hook Executor With Progress Callback
    ///
    /// Verifies that progress callback is called correctly.
    ///
    /// ## Test Scenario
    /// - Creates executor with hooks
    /// - Runs with progress callback
    ///
    /// ## Expected Outcome
    /// - Callback receives expected events
    #[test]
    fn test_hook_executor_with_progress_callback() {
        let config = HooksConfig {
            post_merge: vec!["echo test".to_string()],
            ..Default::default()
        };
        let executor = HookExecutor::new(config);
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new();

        let mut events = Vec::new();
        let callback = |event: HookProgress| {
            events.push(event);
        };

        executor.run_hooks(
            HookTrigger::PostMerge,
            temp_dir.path(),
            &context,
            Some(callback),
        );

        assert_eq!(events.len(), 4); // Starting, CommandStarting, CommandCompleted, Completed
    }
}
