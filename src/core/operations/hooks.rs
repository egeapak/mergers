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

/// What to do when a hook command fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookFailureMode {
    /// Abort the entire workflow.
    Abort,
    /// Continue workflow but emit warning (default).
    #[default]
    Continue,
}

/// Whether to run hooks synchronously or asynchronously.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionMode {
    /// Run synchronously, blocking workflow (default).
    #[default]
    Blocking,
    /// Fire and forget - start hook and continue immediately.
    Async,
}

/// Result of running hooks that informs the caller what action to take.
#[derive(Debug, Clone)]
pub enum HookOutcome {
    /// All hooks succeeded or no hooks configured.
    Success,
    /// Hook failed but configured to continue (warning emitted).
    ContinuedAfterFailure {
        /// The trigger that failed.
        trigger: HookTrigger,
        /// The command that failed.
        command: String,
        /// Error message.
        error: String,
    },
    /// Hook failed and configured to abort workflow.
    Abort {
        /// The trigger that failed.
        trigger: HookTrigger,
        /// The command that failed.
        command: String,
        /// Error message.
        error: String,
    },
    /// Hooks started asynchronously (no result yet).
    Async,
}

impl HookOutcome {
    /// Returns true if the workflow should abort.
    pub fn should_abort(&self) -> bool {
        matches!(self, HookOutcome::Abort { .. })
    }

    /// Returns true if this was a successful outcome.
    pub fn is_success(&self) -> bool {
        matches!(self, HookOutcome::Success | HookOutcome::Async)
    }
}

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

    /// Returns the default failure mode for this trigger.
    ///
    /// Pre-hooks and setup hooks abort by default since failures there
    /// indicate problems that should stop the workflow.
    /// Post-hooks continue by default since they're often informational.
    pub fn default_failure_mode(&self) -> HookFailureMode {
        match self {
            // Setup and validation hooks should abort on failure
            HookTrigger::PostCheckout | HookTrigger::PreCherryPick => HookFailureMode::Abort,
            // Post-operation hooks continue by default
            HookTrigger::PostCherryPick
            | HookTrigger::PostMerge
            | HookTrigger::OnConflict
            | HookTrigger::PostComplete => HookFailureMode::Continue,
        }
    }

    /// Creates a HookTrigger from a config key string.
    pub fn from_config_key(key: &str) -> Option<Self> {
        match key {
            "post_checkout" => Some(HookTrigger::PostCheckout),
            "pre_cherry_pick" => Some(HookTrigger::PreCherryPick),
            "post_cherry_pick" => Some(HookTrigger::PostCherryPick),
            "post_merge" => Some(HookTrigger::PostMerge),
            "on_conflict" => Some(HookTrigger::OnConflict),
            "post_complete" => Some(HookTrigger::PostComplete),
            _ => None,
        }
    }
}

/// Default timeout in seconds for hook commands.
fn default_timeout_secs() -> u64 {
    300
}

/// Extended configuration for a single hook trigger.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HookTriggerConfig {
    /// Commands to run.
    #[serde(default)]
    pub commands: Vec<String>,
    /// What to do on failure (if not set, uses trigger's default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_failure: Option<HookFailureMode>,
    /// Execution mode.
    #[serde(default)]
    pub execution: HookExecutionMode,
    /// Timeout in seconds per command (default: 300).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl HookTriggerConfig {
    /// Creates a new empty hook trigger config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a config from a list of commands with default settings.
    pub fn from_commands(commands: Vec<String>) -> Self {
        Self {
            commands,
            on_failure: None,
            execution: HookExecutionMode::default(),
            timeout_secs: default_timeout_secs(),
        }
    }

    /// Returns true if this config has any commands.
    pub fn has_commands(&self) -> bool {
        !self.commands.is_empty()
    }

    /// Returns the failure mode, using the trigger's default if not set.
    pub fn failure_mode(&self, trigger: HookTrigger) -> HookFailureMode {
        self.on_failure
            .unwrap_or_else(|| trigger.default_failure_mode())
    }
}

/// Helper function for serde to skip serializing empty trigger configs.
fn is_empty_trigger_config(config: &HookTriggerConfig) -> bool {
    !config.has_commands()
}

/// Configuration for hooks.
///
/// Supports both simple format (list of commands) and extended format
/// (with failure mode and execution mode configuration).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HooksConfig {
    /// Commands to run after repository checkout/setup.
    #[serde(
        default,
        skip_serializing_if = "is_empty_trigger_config",
        deserialize_with = "deserialize_hook_trigger_config"
    )]
    pub post_checkout: HookTriggerConfig,

    /// Commands to run before starting cherry-picks.
    #[serde(
        default,
        skip_serializing_if = "is_empty_trigger_config",
        deserialize_with = "deserialize_hook_trigger_config"
    )]
    pub pre_cherry_pick: HookTriggerConfig,

    /// Commands to run after each successful cherry-pick.
    #[serde(
        default,
        skip_serializing_if = "is_empty_trigger_config",
        deserialize_with = "deserialize_hook_trigger_config"
    )]
    pub post_cherry_pick: HookTriggerConfig,

    /// Commands to run after all cherry-picks complete.
    #[serde(
        default,
        skip_serializing_if = "is_empty_trigger_config",
        deserialize_with = "deserialize_hook_trigger_config"
    )]
    pub post_merge: HookTriggerConfig,

    /// Commands to run when a conflict is detected.
    #[serde(
        default,
        skip_serializing_if = "is_empty_trigger_config",
        deserialize_with = "deserialize_hook_trigger_config"
    )]
    pub on_conflict: HookTriggerConfig,

    /// Commands to run after the complete command finishes.
    #[serde(
        default,
        skip_serializing_if = "is_empty_trigger_config",
        deserialize_with = "deserialize_hook_trigger_config"
    )]
    pub post_complete: HookTriggerConfig,
}

/// Custom deserializer that supports both simple (Vec<String>) and extended (HookTriggerConfig) formats.
fn deserialize_hook_trigger_config<'de, D>(deserializer: D) -> Result<HookTriggerConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, MapAccess, SeqAccess, Visitor};

    struct HookTriggerConfigVisitor;

    impl<'de> Visitor<'de> for HookTriggerConfigVisitor {
        type Value = HookTriggerConfig;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a list of commands or a hook trigger config object")
        }

        // Handle simple format: ["cmd1", "cmd2"]
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut commands = Vec::new();
            while let Some(cmd) = seq.next_element::<String>()? {
                commands.push(cmd);
            }
            Ok(HookTriggerConfig::from_commands(commands))
        }

        // Handle extended format: { commands = [...], on_failure = "abort" }
        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut commands = None;
            let mut on_failure = None;
            let mut execution = None;
            let mut timeout_secs = None;

            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "commands" => {
                        commands = Some(map.next_value::<Vec<String>>()?);
                    }
                    "on_failure" => {
                        on_failure = Some(map.next_value::<HookFailureMode>()?);
                    }
                    "execution" => {
                        execution = Some(map.next_value::<HookExecutionMode>()?);
                    }
                    "timeout_secs" => {
                        timeout_secs = Some(map.next_value::<u64>()?);
                    }
                    _ => {
                        return Err(de::Error::unknown_field(
                            &key,
                            &["commands", "on_failure", "execution", "timeout_secs"],
                        ));
                    }
                }
            }

            Ok(HookTriggerConfig {
                commands: commands.unwrap_or_default(),
                on_failure,
                execution: execution.unwrap_or_default(),
                timeout_secs: timeout_secs.unwrap_or_else(default_timeout_secs),
            })
        }
    }

    deserializer.deserialize_any(HookTriggerConfigVisitor)
}

impl HooksConfig {
    /// Creates a new empty hooks configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the configuration for a given trigger.
    pub fn config_for(&self, trigger: HookTrigger) -> &HookTriggerConfig {
        match trigger {
            HookTrigger::PostCheckout => &self.post_checkout,
            HookTrigger::PreCherryPick => &self.pre_cherry_pick,
            HookTrigger::PostCherryPick => &self.post_cherry_pick,
            HookTrigger::PostMerge => &self.post_merge,
            HookTrigger::OnConflict => &self.on_conflict,
            HookTrigger::PostComplete => &self.post_complete,
        }
    }

    /// Returns the commands for a given trigger.
    pub fn commands_for(&self, trigger: HookTrigger) -> &[String] {
        &self.config_for(trigger).commands
    }

    /// Returns true if any hooks are configured.
    pub fn has_hooks(&self) -> bool {
        self.post_checkout.has_commands()
            || self.pre_cherry_pick.has_commands()
            || self.post_cherry_pick.has_commands()
            || self.post_merge.has_commands()
            || self.on_conflict.has_commands()
            || self.post_complete.has_commands()
    }

    /// Returns true if hooks are configured for the given trigger.
    pub fn has_hooks_for(&self, trigger: HookTrigger) -> bool {
        self.config_for(trigger).has_commands()
    }

    /// Merges another hooks config into this one, with other taking precedence.
    ///
    /// If the other config has any hooks for a trigger, they replace this config's hooks.
    pub fn merge(self, other: Self) -> Self {
        Self {
            post_checkout: if other.post_checkout.has_commands() {
                other.post_checkout
            } else {
                self.post_checkout
            },
            pre_cherry_pick: if other.pre_cherry_pick.has_commands() {
                other.pre_cherry_pick
            } else {
                self.pre_cherry_pick
            },
            post_cherry_pick: if other.post_cherry_pick.has_commands() {
                other.post_cherry_pick
            } else {
                self.post_cherry_pick
            },
            post_merge: if other.post_merge.has_commands() {
                other.post_merge
            } else {
                self.post_merge
            },
            on_conflict: if other.on_conflict.has_commands() {
                other.on_conflict
            } else {
                self.on_conflict
            },
            post_complete: if other.post_complete.has_commands() {
                other.post_complete
            } else {
                self.post_complete
            },
        }
    }

    /// Creates a HooksConfig from simple command lists (for backward compatibility).
    pub fn from_simple(
        post_checkout: Vec<String>,
        pre_cherry_pick: Vec<String>,
        post_cherry_pick: Vec<String>,
        post_merge: Vec<String>,
        on_conflict: Vec<String>,
        post_complete: Vec<String>,
    ) -> Self {
        Self {
            post_checkout: HookTriggerConfig::from_commands(post_checkout),
            pre_cherry_pick: HookTriggerConfig::from_commands(pre_cherry_pick),
            post_cherry_pick: HookTriggerConfig::from_commands(post_cherry_pick),
            post_merge: HookTriggerConfig::from_commands(post_merge),
            on_conflict: HookTriggerConfig::from_commands(on_conflict),
            post_complete: HookTriggerConfig::from_commands(post_complete),
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

    /// Runs hooks for a trigger and returns an outcome based on failure mode.
    ///
    /// This method considers the configured failure mode for the trigger and
    /// returns an appropriate `HookOutcome` that tells the caller what action
    /// to take.
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
    /// A `HookOutcome` indicating success, continue-after-failure, abort, or async.
    pub fn run_hooks_with_outcome<F>(
        &self,
        trigger: HookTrigger,
        working_dir: &Path,
        context: &HookContext,
        progress_callback: Option<F>,
    ) -> HookOutcome
    where
        F: FnMut(HookProgress),
    {
        let trigger_config = self.config.config_for(trigger);

        // If no hooks configured, return success
        if !trigger_config.has_commands() {
            return HookOutcome::Success;
        }

        // Handle async execution mode
        if trigger_config.execution == HookExecutionMode::Async {
            // For async mode, we start the hooks and return immediately
            // The actual async spawning is handled by the caller
            return HookOutcome::Async;
        }

        // Run hooks synchronously
        let result = self.run_hooks(trigger, working_dir, context, progress_callback);

        if result.all_succeeded {
            return HookOutcome::Success;
        }

        // Get failure details
        let failure = result.first_failure();
        let (command, error) = match failure {
            Some(f) => (
                f.command.clone(),
                if f.stderr.is_empty() {
                    format!("Command failed with exit code {:?}", f.exit_code)
                } else {
                    f.stderr.clone()
                },
            ),
            None => ("unknown".to_string(), "Unknown error".to_string()),
        };

        // Determine action based on failure mode
        let failure_mode = trigger_config.failure_mode(trigger);

        match failure_mode {
            HookFailureMode::Abort => HookOutcome::Abort {
                trigger,
                command,
                error,
            },
            HookFailureMode::Continue => HookOutcome::ContinuedAfterFailure {
                trigger,
                command,
                error,
            },
        }
    }

    /// Runs hooks for a trigger with outcome, without progress callbacks.
    pub fn run_hooks_with_outcome_simple(
        &self,
        trigger: HookTrigger,
        working_dir: &Path,
        context: &HookContext,
    ) -> HookOutcome {
        self.run_hooks_with_outcome::<fn(HookProgress)>(trigger, working_dir, context, None)
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
    /// - All hook trigger configs are empty
    #[test]
    fn test_hooks_config_default() {
        let config = HooksConfig::default();
        assert!(!config.post_checkout.has_commands());
        assert!(!config.pre_cherry_pick.has_commands());
        assert!(!config.post_cherry_pick.has_commands());
        assert!(!config.post_merge.has_commands());
        assert!(!config.on_conflict.has_commands());
        assert!(!config.post_complete.has_commands());
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
            post_merge: HookTriggerConfig::from_commands(vec!["echo test".to_string()]),
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
            post_merge: HookTriggerConfig::from_commands(vec!["echo test".to_string()]),
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
        let config = HooksConfig::from_simple(
            vec!["echo checkout".to_string()],
            vec!["echo pre".to_string()],
            vec!["echo post".to_string()],
            vec!["echo merge1".to_string(), "echo merge2".to_string()],
            vec!["echo conflict".to_string()],
            vec!["echo complete".to_string()],
        );

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
            post_merge: HookTriggerConfig::from_commands(vec!["base cmd".to_string()]),
            on_conflict: HookTriggerConfig::from_commands(vec!["base conflict".to_string()]),
            ..Default::default()
        };

        let other = HooksConfig {
            post_merge: HookTriggerConfig::from_commands(vec!["other cmd".to_string()]),
            post_complete: HookTriggerConfig::from_commands(vec!["other complete".to_string()]),
            ..Default::default()
        };

        let merged = base.merge(other);

        // Other takes precedence
        assert_eq!(merged.post_merge.commands, vec!["other cmd".to_string()]);
        // Base kept when other is empty
        assert_eq!(
            merged.on_conflict.commands,
            vec!["base conflict".to_string()]
        );
        // Other added
        assert_eq!(
            merged.post_complete.commands,
            vec!["other complete".to_string()]
        );
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
            post_merge: HookTriggerConfig::from_commands(vec!["echo hello".to_string()]),
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
            post_merge: HookTriggerConfig::from_commands(vec![
                "echo first".to_string(),
                "echo second".to_string(),
            ]),
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
            post_merge: HookTriggerConfig::from_commands(vec![
                "exit 1".to_string(), // This fails
                "echo should_not_run".to_string(),
            ]),
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
            post_merge: HookTriggerConfig::from_commands(vec!["echo $MERGERS_VERSION".to_string()]),
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
            post_merge: HookTriggerConfig::from_commands(vec!["cargo test".to_string()]),
            on_conflict: HookTriggerConfig::from_commands(vec!["git status".to_string()]),
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

        assert_eq!(
            config.post_checkout.commands,
            vec!["npm install".to_string()]
        );
        assert_eq!(
            config.post_merge.commands,
            vec!["cargo test".to_string(), "cargo build".to_string()]
        );
        assert_eq!(config.on_conflict.commands, vec!["git status".to_string()]);
        assert!(!config.pre_cherry_pick.has_commands());
        assert!(!config.post_cherry_pick.has_commands());
        assert!(!config.post_complete.has_commands());
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
            post_merge: HookTriggerConfig::from_commands(vec!["echo test".to_string()]),
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

    /// # Hook Trigger Default Failure Mode
    ///
    /// Verifies that hook triggers have sensible default failure modes.
    ///
    /// ## Test Scenario
    /// - Checks default failure mode for each trigger
    ///
    /// ## Expected Outcome
    /// - Pre-hooks abort, post-hooks continue
    #[test]
    fn test_hook_trigger_default_failure_mode() {
        // Pre-hooks should abort by default
        assert_eq!(
            HookTrigger::PostCheckout.default_failure_mode(),
            HookFailureMode::Abort
        );
        assert_eq!(
            HookTrigger::PreCherryPick.default_failure_mode(),
            HookFailureMode::Abort
        );

        // Post-hooks should continue by default
        assert_eq!(
            HookTrigger::PostCherryPick.default_failure_mode(),
            HookFailureMode::Continue
        );
        assert_eq!(
            HookTrigger::PostMerge.default_failure_mode(),
            HookFailureMode::Continue
        );
        assert_eq!(
            HookTrigger::OnConflict.default_failure_mode(),
            HookFailureMode::Continue
        );
        assert_eq!(
            HookTrigger::PostComplete.default_failure_mode(),
            HookFailureMode::Continue
        );
    }

    /// # Hook Trigger From Config Key
    ///
    /// Verifies that HookTrigger can be created from config keys.
    ///
    /// ## Test Scenario
    /// - Creates triggers from valid config keys
    ///
    /// ## Expected Outcome
    /// - Valid keys return Some(trigger), invalid returns None
    #[test]
    fn test_hook_trigger_from_config_key() {
        assert_eq!(
            HookTrigger::from_config_key("post_checkout"),
            Some(HookTrigger::PostCheckout)
        );
        assert_eq!(
            HookTrigger::from_config_key("pre_cherry_pick"),
            Some(HookTrigger::PreCherryPick)
        );
        assert_eq!(HookTrigger::from_config_key("invalid"), None);
    }

    /// # Hook Outcome Methods
    ///
    /// Verifies HookOutcome helper methods work correctly.
    ///
    /// ## Test Scenario
    /// - Tests should_abort and is_success for each variant
    ///
    /// ## Expected Outcome
    /// - Methods return correct values
    #[test]
    fn test_hook_outcome_methods() {
        assert!(!HookOutcome::Success.should_abort());
        assert!(HookOutcome::Success.is_success());

        assert!(!HookOutcome::Async.should_abort());
        assert!(HookOutcome::Async.is_success());

        let continued = HookOutcome::ContinuedAfterFailure {
            trigger: HookTrigger::PostMerge,
            command: "test".to_string(),
            error: "error".to_string(),
        };
        assert!(!continued.should_abort());
        assert!(!continued.is_success());

        let abort = HookOutcome::Abort {
            trigger: HookTrigger::PreCherryPick,
            command: "test".to_string(),
            error: "error".to_string(),
        };
        assert!(abort.should_abort());
        assert!(!abort.is_success());
    }

    /// # Hook Outcome With Failure Mode Abort
    ///
    /// Verifies run_hooks_with_outcome returns Abort when failure mode is Abort.
    ///
    /// ## Test Scenario
    /// - Creates config with failing command and Abort failure mode
    /// - Runs hooks
    ///
    /// ## Expected Outcome
    /// - Returns HookOutcome::Abort
    #[test]
    fn test_hook_outcome_abort() {
        let config = HooksConfig {
            pre_cherry_pick: HookTriggerConfig {
                commands: vec!["exit 1".to_string()],
                on_failure: Some(HookFailureMode::Abort),
                ..Default::default()
            },
            ..Default::default()
        };
        let executor = HookExecutor::new(config);
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new();

        let outcome = executor.run_hooks_with_outcome_simple(
            HookTrigger::PreCherryPick,
            temp_dir.path(),
            &context,
        );

        assert!(outcome.should_abort());
        if let HookOutcome::Abort { command, .. } = outcome {
            assert_eq!(command, "exit 1");
        } else {
            panic!("Expected Abort outcome");
        }
    }

    /// # Hook Outcome With Failure Mode Continue
    ///
    /// Verifies run_hooks_with_outcome returns ContinuedAfterFailure when mode is Continue.
    ///
    /// ## Test Scenario
    /// - Creates config with failing command and Continue failure mode
    /// - Runs hooks
    ///
    /// ## Expected Outcome
    /// - Returns HookOutcome::ContinuedAfterFailure
    #[test]
    fn test_hook_outcome_continue() {
        let config = HooksConfig {
            post_merge: HookTriggerConfig {
                commands: vec!["exit 1".to_string()],
                on_failure: Some(HookFailureMode::Continue),
                ..Default::default()
            },
            ..Default::default()
        };
        let executor = HookExecutor::new(config);
        let temp_dir = TempDir::new().unwrap();
        let context = HookContext::new();

        let outcome = executor.run_hooks_with_outcome_simple(
            HookTrigger::PostMerge,
            temp_dir.path(),
            &context,
        );

        assert!(!outcome.should_abort());
        assert!(matches!(outcome, HookOutcome::ContinuedAfterFailure { .. }));
    }

    /// # Extended Config TOML Parsing
    ///
    /// Verifies that extended hook configuration format parses correctly.
    ///
    /// ## Test Scenario
    /// - Parses TOML with extended format (commands, on_failure, etc)
    ///
    /// ## Expected Outcome
    /// - Config has correct values including failure mode
    #[test]
    fn test_extended_config_toml_parsing() {
        let toml_str = r#"
[post_checkout]
commands = ["npm install"]
on_failure = "abort"

[post_merge]
commands = ["cargo test"]
on_failure = "continue"
"#;

        let config: HooksConfig = toml::from_str(toml_str).unwrap();

        assert_eq!(
            config.post_checkout.commands,
            vec!["npm install".to_string()]
        );
        assert_eq!(
            config.post_checkout.on_failure,
            Some(HookFailureMode::Abort)
        );

        assert_eq!(config.post_merge.commands, vec!["cargo test".to_string()]);
        assert_eq!(
            config.post_merge.on_failure,
            Some(HookFailureMode::Continue)
        );
    }
}
