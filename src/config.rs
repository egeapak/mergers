//! Configuration management for mergers.
//!
//! This module handles loading configuration from multiple sources:
//! - TOML configuration files following XDG Base Directory specification
//! - Environment variables
//! - Git remote detection for Azure DevOps repositories
//!
//! ## Example
//!
//! ```rust
//! use mergers::Config;
//!
//! // Load configuration from file, with fallback to defaults
//! let config = Config::load_from_file().unwrap();
//! println!("Dev branch: {:?}", config.dev_branch);
//!
//! // Load from environment variables
//! let env_config = Config::load_from_env();
//!
//! // Merge configurations (env takes precedence)
//! let merged = config.merge(env_config);
//! ```

use crate::{git_config, models::SharedArgs, parsed_property::ParsedProperty};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Temporary struct for deserializing TOML configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConfigFile {
    pub organization: Option<String>,
    pub project: Option<String>,
    pub repository: Option<String>,
    pub pat: Option<String>,
    pub dev_branch: Option<String>,
    pub target_branch: Option<String>,
    pub local_repo: Option<String>,
    pub work_item_state: Option<String>,
    pub parallel_limit: Option<usize>,
    pub max_concurrent_network: Option<usize>,
    pub max_concurrent_processing: Option<usize>,
    pub tag_prefix: Option<String>,
    pub run_hooks: Option<bool>,
    // UI Settings
    pub show_dependency_highlights: Option<bool>,
    pub show_work_item_highlights: Option<bool>,
    // Release Notes Settings
    pub repo_aliases: Option<std::collections::HashMap<String, String>>,
}

/// Application configuration assembled from CLI arguments, environment variables, config file, and defaults.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Azure DevOps organization name.
    pub organization: Option<ParsedProperty<String>>,
    /// Azure DevOps project name.
    pub project: Option<ParsedProperty<String>>,
    /// Azure DevOps repository name.
    pub repository: Option<ParsedProperty<String>>,
    /// Personal access token for authenticating with Azure DevOps.
    pub pat: Option<ParsedProperty<String>>,
    /// Name of the development branch to fetch pull requests from.
    pub dev_branch: Option<ParsedProperty<String>>,
    /// Name of the target branch to merge pull requests into.
    pub target_branch: Option<ParsedProperty<String>>,
    /// Path to a local repository to use instead of cloning.
    pub local_repo: Option<ParsedProperty<String>>,
    /// Work item state to set after a successful merge operation.
    pub work_item_state: Option<ParsedProperty<String>>,
    /// Maximum number of parallel operations for API calls.
    pub parallel_limit: Option<ParsedProperty<usize>>,
    /// Maximum number of concurrent network requests.
    pub max_concurrent_network: Option<ParsedProperty<usize>>,
    /// Maximum number of concurrent processing tasks.
    pub max_concurrent_processing: Option<ParsedProperty<usize>>,
    /// Prefix applied to git tags created during merge operations.
    pub tag_prefix: Option<ParsedProperty<String>>,
    /// Whether to run git hooks during merge operations.
    pub run_hooks: Option<ParsedProperty<bool>>,
    /// Whether to highlight PR dependency relationships in the TUI.
    pub show_dependency_highlights: Option<ParsedProperty<bool>>,
    /// Whether to highlight work item relationships in the TUI.
    pub show_work_item_highlights: Option<ParsedProperty<bool>>,
    /// Repository aliases (e.g., "api" -> "/path/to/api-backend")
    pub repo_aliases: Option<ParsedProperty<std::collections::HashMap<String, String>>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            organization: None,
            project: None,
            repository: None,
            pat: None,
            dev_branch: Some(ParsedProperty::Default("dev".to_string())),
            target_branch: Some(ParsedProperty::Default("next".to_string())),
            local_repo: None,
            work_item_state: Some(ParsedProperty::Default("Next Merged".to_string())),
            parallel_limit: Some(ParsedProperty::Default(300)),
            max_concurrent_network: Some(ParsedProperty::Default(100)),
            max_concurrent_processing: Some(ParsedProperty::Default(10)),
            tag_prefix: Some(ParsedProperty::Default("merged-".to_string())),
            run_hooks: Some(ParsedProperty::Default(false)),
            // UI Settings - both enabled by default
            show_dependency_highlights: Some(ParsedProperty::Default(true)),
            show_work_item_highlights: Some(ParsedProperty::Default(true)),
            // Release Notes Settings
            repo_aliases: None,
        }
    }
}

impl Config {
    /// Load configuration from XDG config directory
    #[must_use = "this returns the loaded configuration which should be used"]
    pub fn load_from_file() -> Result<Self> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let config_content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

        let config_file: ConfigFile = toml::from_str(&config_content)
            .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;

        Ok(Self {
            organization: config_file
                .organization
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            project: config_file
                .project
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            repository: config_file
                .repository
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            pat: config_file
                .pat
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            dev_branch: config_file
                .dev_branch
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            target_branch: config_file
                .target_branch
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            local_repo: config_file
                .local_repo
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            work_item_state: config_file
                .work_item_state
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            parallel_limit: config_file
                .parallel_limit
                .map(|v| ParsedProperty::File(v, config_path.clone(), v.to_string())),
            max_concurrent_network: config_file
                .max_concurrent_network
                .map(|v| ParsedProperty::File(v, config_path.clone(), v.to_string())),
            max_concurrent_processing: config_file
                .max_concurrent_processing
                .map(|v| ParsedProperty::File(v, config_path.clone(), v.to_string())),
            tag_prefix: config_file
                .tag_prefix
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), v)),
            run_hooks: config_file
                .run_hooks
                .map(|v| ParsedProperty::File(v, config_path.clone(), v.to_string())),
            show_dependency_highlights: config_file
                .show_dependency_highlights
                .map(|v| ParsedProperty::File(v, config_path.clone(), v.to_string())),
            show_work_item_highlights: config_file
                .show_work_item_highlights
                .map(|v| ParsedProperty::File(v, config_path.clone(), v.to_string())),
            repo_aliases: config_file
                .repo_aliases
                .map(|v| ParsedProperty::File(v.clone(), config_path.clone(), format!("{:?}", v))),
        })
    }

    /// Detect configuration from git remote.
    ///
    /// For Azure DevOps URLs, extracts organization, project, and repository.
    /// For other Git URLs (GitHub, GitLab, etc.), uses the repository name as
    /// the default project value.
    pub fn detect_from_git_remote<P: AsRef<std::path::Path>>(repo_path: P) -> Self {
        let repo_path_ref = repo_path.as_ref();

        // First try to get the remote URL for better error context
        let remote_url = std::process::Command::new("git")
            .current_dir(repo_path_ref)
            .args(["remote", "get-url", "origin"])
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        // First, try Azure DevOps config
        if let Ok(Some(azure_config)) = git_config::detect_azure_devops_config(repo_path_ref) {
            return Self {
                organization: Some(ParsedProperty::Git(
                    azure_config.organization,
                    remote_url.clone(),
                )),
                project: Some(ParsedProperty::Git(
                    azure_config.project,
                    remote_url.clone(),
                )),
                repository: Some(ParsedProperty::Git(azure_config.repository, remote_url)),
                pat: None,
                dev_branch: None,
                target_branch: None,
                local_repo: None,
                work_item_state: None,
                parallel_limit: None,
                max_concurrent_network: None,
                max_concurrent_processing: None,
                tag_prefix: None,
                run_hooks: None,
                show_dependency_highlights: None,
                show_work_item_highlights: None,
                repo_aliases: None,
            };
        }

        // Try generic Git config (GitHub, GitLab, etc.)
        // Use the repository name as the project default
        if let Ok(Some(generic_config)) = git_config::detect_generic_git_config(repo_path_ref) {
            return Self {
                organization: None,
                // Use repository name as project default for non-Azure DevOps URLs
                project: Some(ParsedProperty::Git(
                    generic_config.repository.clone(),
                    remote_url.clone(),
                )),
                repository: Some(ParsedProperty::Git(generic_config.repository, remote_url)),
                pat: None,
                dev_branch: None,
                target_branch: None,
                local_repo: None,
                work_item_state: None,
                parallel_limit: None,
                max_concurrent_network: None,
                max_concurrent_processing: None,
                tag_prefix: None,
                run_hooks: None,
                show_dependency_highlights: None,
                show_work_item_highlights: None,
                repo_aliases: None,
            };
        }

        Self::default()
    }

    /// Load configuration from environment variables
    pub fn load_from_env() -> Self {
        Self {
            organization: std::env::var("MERGERS_ORGANIZATION")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            project: std::env::var("MERGERS_PROJECT")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            repository: std::env::var("MERGERS_REPOSITORY")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            pat: std::env::var("MERGERS_PAT")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            dev_branch: std::env::var("MERGERS_DEV_BRANCH")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            target_branch: std::env::var("MERGERS_TARGET_BRANCH")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            local_repo: std::env::var("MERGERS_LOCAL_REPO")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            work_item_state: std::env::var("MERGERS_WORK_ITEM_STATE")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            parallel_limit: std::env::var("MERGERS_PARALLEL_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok().map(|v| ParsedProperty::Env(v, s))),
            max_concurrent_network: std::env::var("MERGERS_MAX_CONCURRENT_NETWORK")
                .ok()
                .and_then(|s| s.parse().ok().map(|v| ParsedProperty::Env(v, s))),
            max_concurrent_processing: std::env::var("MERGERS_MAX_CONCURRENT_PROCESSING")
                .ok()
                .and_then(|s| s.parse().ok().map(|v| ParsedProperty::Env(v, s))),
            tag_prefix: std::env::var("MERGERS_TAG_PREFIX")
                .ok()
                .map(|v| ParsedProperty::Env(v.clone(), v)),
            run_hooks: std::env::var("MERGERS_RUN_HOOKS").ok().and_then(|s| {
                s.parse::<bool>()
                    .ok()
                    .map(|v| ParsedProperty::Env(v, s.clone()))
            }),
            show_dependency_highlights: std::env::var("MERGERS_SHOW_DEPENDENCY_HIGHLIGHTS")
                .ok()
                .and_then(|s| {
                    s.parse::<bool>()
                        .ok()
                        .map(|v| ParsedProperty::Env(v, s.clone()))
                }),
            show_work_item_highlights: std::env::var("MERGERS_SHOW_WORK_ITEM_HIGHLIGHTS")
                .ok()
                .and_then(|s| {
                    s.parse::<bool>()
                        .ok()
                        .map(|v| ParsedProperty::Env(v, s.clone()))
                }),
            // repo_aliases is configured via file only, not environment variables
            repo_aliases: None,
        }
    }

    /// Get the XDG config directory path for mergers
    fn get_config_path() -> Result<PathBuf> {
        // Use XDG_CONFIG_HOME if set, otherwise ~/.config
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .expect("Could not determine home directory")
                    .join(".config")
            });

        let mergers_config_dir = config_dir.join("mergers");

        // Create config directory if it doesn't exist
        if !mergers_config_dir.exists() {
            fs::create_dir_all(&mergers_config_dir).with_context(|| {
                format!(
                    "Failed to create config directory: {}",
                    mergers_config_dir.display()
                )
            })?;
        }

        Ok(mergers_config_dir.join("config.toml"))
    }

    /// Merge this config with another, preferring values from other when they exist
    pub fn merge(self, other: Self) -> Self {
        Self {
            organization: other.organization.or(self.organization),
            project: other.project.or(self.project),
            repository: other.repository.or(self.repository),
            pat: other.pat.or(self.pat),
            dev_branch: other.dev_branch.or(self.dev_branch),
            target_branch: other.target_branch.or(self.target_branch),
            local_repo: other.local_repo.or(self.local_repo),
            work_item_state: other.work_item_state.or(self.work_item_state),
            parallel_limit: other.parallel_limit.or(self.parallel_limit),
            max_concurrent_network: other.max_concurrent_network.or(self.max_concurrent_network),
            max_concurrent_processing: other
                .max_concurrent_processing
                .or(self.max_concurrent_processing),
            tag_prefix: other.tag_prefix.or(self.tag_prefix),
            run_hooks: other.run_hooks.or(self.run_hooks),
            show_dependency_highlights: other
                .show_dependency_highlights
                .or(self.show_dependency_highlights),
            show_work_item_highlights: other
                .show_work_item_highlights
                .or(self.show_work_item_highlights),
            repo_aliases: other.repo_aliases.or(self.repo_aliases),
        }
    }

    /// Create a sample config file for user reference
    #[must_use = "this operation can fail and the result should be checked"]
    pub fn create_sample_config() -> Result<()> {
        let config_path = Self::get_config_path()?;

        // Don't overwrite existing config
        if config_path.exists() {
            return Ok(());
        }

        let sample_config = r#"# Mergers Configuration File
# This file follows the XDG Base Directory specification
# Location: ~/.config/mergers/config.toml (Linux/macOS) or %APPDATA%\mergers\config.toml (Windows)

# Azure DevOps organization (required)
# organization = "your-organization"

# Azure DevOps project (required)
# project = "your-project"

# Repository name (required)
# repository = "your-repository"

# Personal Access Token (required, but consider using environment variable MERGERS_PAT instead)
# pat = "your-pat-token"

# Development branch name (optional, defaults to "dev")
dev_branch = "dev"

# Target branch name (optional, defaults to "next")
target_branch = "next"

# Local repository path (optional, uses git worktree instead of cloning)
# local_repo = "/path/to/your/local/repo"

# Target state for work items after successful merge (optional, defaults to "Next Merged")
work_item_state = "Next Merged"

# Maximum number of parallel operations for API calls (optional, defaults to 300)
parallel_limit = 300

# Maximum number of concurrent network operations (optional, defaults to 100)
max_concurrent_network = 100

# Maximum number of concurrent processing operations (optional, defaults to 10)
max_concurrent_processing = 10

# UI Settings
# Show dependency highlighting in PR selection (optional, defaults to true)
show_dependency_highlights = true

# Show work item relationship highlighting in PR selection (optional, defaults to true)
show_work_item_highlights = true

# Repository aliases for quick access
# Maps short names to full paths (usable with any command)
# [repo_aliases]
# api = "/path/to/api-backend"
# web = "/path/to/web-frontend"
"#;

        fs::write(&config_path, sample_config).with_context(|| {
            format!(
                "Failed to write sample config to: {}",
                config_path.display()
            )
        })?;

        println!("Sample config created at: {}", config_path.display());
        Ok(())
    }

    /// Save UI settings to the config file.
    ///
    /// This method reads the existing config file (if any), updates only the UI settings,
    /// and writes the result back to preserve user's other settings.
    pub fn save_ui_settings(
        show_dependency_highlights: bool,
        show_work_item_highlights: bool,
    ) -> Result<()> {
        let config_path = Self::get_config_path()?;

        // Read existing config file or start with empty
        let mut config_file: ConfigFile = if config_path.exists() {
            let content = fs::read_to_string(&config_path).with_context(|| {
                format!("Failed to read config file: {}", config_path.display())
            })?;
            toml::from_str(&content).unwrap_or_default()
        } else {
            ConfigFile::default()
        };

        // Update UI settings
        config_file.show_dependency_highlights = Some(show_dependency_highlights);
        config_file.show_work_item_highlights = Some(show_work_item_highlights);

        // Serialize and write
        let toml_string =
            toml::to_string_pretty(&config_file).with_context(|| "Failed to serialize config")?;

        fs::write(&config_path, toml_string)
            .with_context(|| format!("Failed to write config to: {}", config_path.display()))?;

        Ok(())
    }

    /// Build a Config from SharedArgs CLI values.
    ///
    /// Converts SharedArgs fields into `ParsedProperty::Cli` variants.
    /// Command-specific fields (work_item_state, run_hooks, etc.) are left as None
    /// and should be set by the caller if needed.
    pub fn from_shared_args(shared: &SharedArgs) -> Self {
        let cli_local_repo = shared.path.as_ref().or(shared.local_repo.as_ref());
        Config {
            organization: shared
                .organization
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            project: shared
                .project
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            repository: shared
                .repository
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            pat: shared
                .pat
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            dev_branch: shared
                .dev_branch
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            target_branch: shared
                .target_branch
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            local_repo: cli_local_repo.map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            parallel_limit: shared
                .parallel_limit
                .map(|v| ParsedProperty::Cli(v, v.to_string())),
            max_concurrent_network: shared
                .max_concurrent_network
                .map(|v| ParsedProperty::Cli(v, v.to_string())),
            max_concurrent_processing: shared
                .max_concurrent_processing
                .map(|v| ParsedProperty::Cli(v, v.to_string())),
            tag_prefix: shared
                .tag_prefix
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            // Command-specific fields: not set from SharedArgs
            work_item_state: None,
            run_hooks: None,
            // UI settings: not set via CLI
            show_dependency_highlights: None,
            show_work_item_highlights: None,
            // Repo aliases: not set via CLI
            repo_aliases: None,
        }
    }
}

/// Resolve repository path from alias or path.
///
/// # Arguments
///
/// * `path_or_alias` - Optional path or alias (e.g., "th", "/path/to/repo")
/// * `aliases` - Map of alias names to paths from config
///
/// # Returns
///
/// Resolved PathBuf to the repository.
pub fn resolve_repo_path(
    path_or_alias: Option<&str>,
    aliases: &Option<HashMap<String, String>>,
) -> Result<PathBuf> {
    match path_or_alias {
        None => std::env::current_dir().context("Failed to get current directory"),
        Some(input) => {
            if let Some(alias_map) = aliases
                && let Some(path) = alias_map.get(input)
            {
                return Ok(PathBuf::from(path));
            }

            let path = PathBuf::from(input);
            if path.exists() {
                Ok(path)
            } else {
                anyhow::bail!(
                    "Path '{}' does not exist. If this is an alias, configure it in ~/.config/mergers/config.toml under [repo_aliases]",
                    input
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::file_serial;
    use std::env;
    use tempfile::TempDir;

    /// # Config Default Values
    ///
    /// Tests that the default configuration contains expected values.
    ///
    /// ## Test Scenario
    /// - Creates a default Config instance
    /// - Validates all default field values
    ///
    /// ## Expected Outcome
    /// - Default values match expected configuration
    /// - All optional fields have sensible defaults
    #[test]
    fn test_config_default() {
        let config = Config::default();

        assert_eq!(config.organization, None);
        assert_eq!(config.project, None);
        assert_eq!(config.repository, None);
        assert_eq!(config.pat, None);
        assert_eq!(
            config.dev_branch,
            Some(ParsedProperty::Default("dev".to_string()))
        );
        assert_eq!(
            config.target_branch,
            Some(ParsedProperty::Default("next".to_string()))
        );
        assert_eq!(config.local_repo, None);
        assert_eq!(
            config.work_item_state,
            Some(ParsedProperty::Default("Next Merged".to_string()))
        );
        assert_eq!(config.parallel_limit, Some(ParsedProperty::Default(300)));
        assert_eq!(
            config.max_concurrent_network,
            Some(ParsedProperty::Default(100))
        );
        assert_eq!(
            config.max_concurrent_processing,
            Some(ParsedProperty::Default(10))
        );
        assert_eq!(
            config.tag_prefix,
            Some(ParsedProperty::Default("merged-".to_string()))
        );
    }

    /// # Load Config from Environment Variables (All Set)
    ///
    /// Tests loading configuration when all environment variables are present.
    ///
    /// ## Test Scenario
    /// - Sets all possible MERGERS_* environment variables
    /// - Loads configuration from environment
    ///
    /// ## Expected Outcome
    /// - All environment variables are correctly parsed
    /// - Configuration reflects all provided environment values
    #[test]
    #[file_serial(env_tests)]
    fn test_load_from_env_all_variables() {
        // Set up environment variables
        unsafe {
            env::set_var("MERGERS_ORGANIZATION", "test-org");
        }
        unsafe {
            env::set_var("MERGERS_PROJECT", "test-project");
        }
        unsafe {
            env::set_var("MERGERS_REPOSITORY", "test-repo");
        }
        unsafe {
            env::set_var("MERGERS_PAT", "test-pat");
        }
        unsafe {
            env::set_var("MERGERS_DEV_BRANCH", "develop");
        }
        unsafe {
            env::set_var("MERGERS_TARGET_BRANCH", "main");
        }
        unsafe {
            env::set_var("MERGERS_LOCAL_REPO", "/tmp/repo");
        }
        unsafe {
            env::set_var("MERGERS_WORK_ITEM_STATE", "Done");
        }
        unsafe {
            env::set_var("MERGERS_PARALLEL_LIMIT", "500");
        }
        unsafe {
            env::set_var("MERGERS_MAX_CONCURRENT_NETWORK", "200");
        }
        unsafe {
            env::set_var("MERGERS_MAX_CONCURRENT_PROCESSING", "20");
        }
        unsafe {
            env::set_var("MERGERS_TAG_PREFIX", "release-");
        }

        let config = Config::load_from_env();

        assert_eq!(
            config.organization,
            Some(ParsedProperty::Env(
                "test-org".to_string(),
                "test-org".to_string()
            ))
        );
        assert_eq!(
            config.project,
            Some(ParsedProperty::Env(
                "test-project".to_string(),
                "test-project".to_string()
            ))
        );
        assert_eq!(
            config.repository,
            Some(ParsedProperty::Env(
                "test-repo".to_string(),
                "test-repo".to_string()
            ))
        );
        assert_eq!(
            config.pat,
            Some(ParsedProperty::Env(
                "test-pat".to_string(),
                "test-pat".to_string()
            ))
        );
        assert_eq!(
            config.dev_branch,
            Some(ParsedProperty::Env(
                "develop".to_string(),
                "develop".to_string()
            ))
        );
        assert_eq!(
            config.target_branch,
            Some(ParsedProperty::Env("main".to_string(), "main".to_string()))
        );
        assert_eq!(
            config.local_repo,
            Some(ParsedProperty::Env(
                "/tmp/repo".to_string(),
                "/tmp/repo".to_string()
            ))
        );
        assert_eq!(
            config.work_item_state,
            Some(ParsedProperty::Env("Done".to_string(), "Done".to_string()))
        );
        assert_eq!(
            config.parallel_limit,
            Some(ParsedProperty::Env(500, "500".to_string()))
        );
        assert_eq!(
            config.max_concurrent_network,
            Some(ParsedProperty::Env(200, "200".to_string()))
        );
        assert_eq!(
            config.max_concurrent_processing,
            Some(ParsedProperty::Env(20, "20".to_string()))
        );
        assert_eq!(
            config.tag_prefix,
            Some(ParsedProperty::Env(
                "release-".to_string(),
                "release-".to_string()
            ))
        );

        // Clean up
        unsafe {
            env::remove_var("MERGERS_ORGANIZATION");
        }
        unsafe {
            env::remove_var("MERGERS_PROJECT");
        }
        unsafe {
            env::remove_var("MERGERS_REPOSITORY");
        }
        unsafe {
            env::remove_var("MERGERS_PAT");
        }
        unsafe {
            env::remove_var("MERGERS_DEV_BRANCH");
        }
        unsafe {
            env::remove_var("MERGERS_TARGET_BRANCH");
        }
        unsafe {
            env::remove_var("MERGERS_LOCAL_REPO");
        }
        unsafe {
            env::remove_var("MERGERS_WORK_ITEM_STATE");
        }
        unsafe {
            env::remove_var("MERGERS_PARALLEL_LIMIT");
        }
        unsafe {
            env::remove_var("MERGERS_MAX_CONCURRENT_NETWORK");
        }
        unsafe {
            env::remove_var("MERGERS_MAX_CONCURRENT_PROCESSING");
        }
        unsafe {
            env::remove_var("MERGERS_TAG_PREFIX");
        }
    }

    /// # Load Config from Environment Variables (None Set)
    ///
    /// Tests loading configuration when no environment variables are set.
    ///
    /// ## Test Scenario
    /// - Clears all relevant environment variables
    /// - Attempts to load configuration from environment
    ///
    /// ## Expected Outcome
    /// - Returns empty/default configuration
    /// - No errors occur when environment variables are missing
    #[test]
    #[file_serial(env_tests)]
    fn test_load_from_env_no_variables() {
        // Ensure no relevant env vars are set - clean up from other tests
        unsafe {
            env::remove_var("MERGERS_ORGANIZATION");
            env::remove_var("MERGERS_PROJECT");
            env::remove_var("MERGERS_REPOSITORY");
            env::remove_var("MERGERS_PAT");
            env::remove_var("MERGERS_DEV_BRANCH");
            env::remove_var("MERGERS_TARGET_BRANCH");
            env::remove_var("MERGERS_LOCAL_REPO");
            env::remove_var("MERGERS_WORK_ITEM_STATE");
            env::remove_var("MERGERS_PARALLEL_LIMIT");
            env::remove_var("MERGERS_MAX_CONCURRENT_NETWORK");
            env::remove_var("MERGERS_MAX_CONCURRENT_PROCESSING");
            env::remove_var("MERGERS_TAG_PREFIX");
        }

        let config = Config::load_from_env();

        assert_eq!(config.organization, None);
        assert_eq!(config.project, None);
        assert_eq!(config.repository, None);
        assert_eq!(config.pat, None);
        assert_eq!(config.dev_branch, None);
        assert_eq!(config.target_branch, None);
        assert_eq!(config.local_repo, None);
        assert_eq!(config.work_item_state, None);
        assert_eq!(config.parallel_limit, None);
        assert_eq!(config.max_concurrent_network, None);
        assert_eq!(config.max_concurrent_processing, None);
        assert_eq!(config.tag_prefix, None);
    }

    /// # Load Config from Environment (Invalid Numeric Values)
    ///
    /// Tests handling of invalid numeric values in environment variables.
    ///
    /// ## Test Scenario
    /// - Sets numeric environment variables to invalid values
    /// - Attempts to load and parse configuration
    ///
    /// ## Expected Outcome
    /// - Invalid numeric values are handled gracefully
    /// - Configuration uses defaults for unparseable numbers
    #[test]
    #[file_serial(env_tests)]
    fn test_load_from_env_invalid_numeric_values() {
        unsafe {
            env::set_var("MERGERS_PARALLEL_LIMIT", "not-a-number");
        }
        unsafe {
            env::set_var("MERGERS_MAX_CONCURRENT_NETWORK", "invalid");
        }
        unsafe {
            env::set_var("MERGERS_MAX_CONCURRENT_PROCESSING", "bad");
        }

        let config = Config::load_from_env();

        assert_eq!(config.parallel_limit, None);
        assert_eq!(config.max_concurrent_network, None);
        assert_eq!(config.max_concurrent_processing, None);

        // Clean up
        unsafe {
            env::remove_var("MERGERS_PARALLEL_LIMIT");
        }
        unsafe {
            env::remove_var("MERGERS_MAX_CONCURRENT_NETWORK");
        }
        unsafe {
            env::remove_var("MERGERS_MAX_CONCURRENT_PROCESSING");
        }
    }

    /// # Config Merge (Other Takes Precedence)
    ///
    /// Tests configuration merging where the other config takes precedence.
    ///
    /// ## Test Scenario
    /// - Creates base and override configurations
    /// - Merges configurations with override taking precedence
    ///
    /// ## Expected Outcome
    /// - Override values replace base values
    /// - Merge precedence rules are correctly applied
    #[test]
    fn test_config_merge_other_takes_precedence() {
        let base = Config {
            organization: Some(ParsedProperty::Default("base-org".to_string())),
            project: Some(ParsedProperty::Default("base-project".to_string())),
            repository: None,
            pat: Some(ParsedProperty::Default("base-pat".to_string())),
            dev_branch: Some(ParsedProperty::Default("base-dev".to_string())),
            target_branch: None,
            local_repo: None,
            work_item_state: Some(ParsedProperty::Default("base-state".to_string())),
            parallel_limit: Some(ParsedProperty::Default(100)),
            max_concurrent_network: None,
            max_concurrent_processing: Some(ParsedProperty::Default(5)),
            tag_prefix: Some(ParsedProperty::Default("base-".to_string())),
            run_hooks: None,
            show_dependency_highlights: None,
            show_work_item_highlights: None,
            repo_aliases: None,
        };

        let other = Config {
            organization: Some(ParsedProperty::Default("other-org".to_string())),
            project: None,
            repository: Some(ParsedProperty::Default("other-repo".to_string())),
            pat: None,
            dev_branch: Some(ParsedProperty::Default("other-dev".to_string())),
            target_branch: Some(ParsedProperty::Default("other-target".to_string())),
            local_repo: Some(ParsedProperty::Default("/other/path".to_string())),
            work_item_state: None,
            parallel_limit: None,
            max_concurrent_network: Some(ParsedProperty::Default(200)),
            max_concurrent_processing: Some(ParsedProperty::Default(15)),
            tag_prefix: None,
            run_hooks: None,
            show_dependency_highlights: None,
            show_work_item_highlights: None,
            repo_aliases: None,
        };

        let merged = base.merge(other);

        // Other values should take precedence when present
        assert_eq!(
            merged.organization,
            Some(ParsedProperty::Default("other-org".to_string()))
        );
        assert_eq!(
            merged.project,
            Some(ParsedProperty::Default("base-project".to_string()))
        ); // base kept when other is None
        assert_eq!(
            merged.repository,
            Some(ParsedProperty::Default("other-repo".to_string()))
        );
        assert_eq!(
            merged.pat,
            Some(ParsedProperty::Default("base-pat".to_string()))
        ); // base kept when other is None
        assert_eq!(
            merged.dev_branch,
            Some(ParsedProperty::Default("other-dev".to_string()))
        );
        assert_eq!(
            merged.target_branch,
            Some(ParsedProperty::Default("other-target".to_string()))
        );
        assert_eq!(
            merged.local_repo,
            Some(ParsedProperty::Default("/other/path".to_string()))
        );
        assert_eq!(
            merged.work_item_state,
            Some(ParsedProperty::Default("base-state".to_string()))
        ); // base kept when other is None
        assert_eq!(merged.parallel_limit, Some(ParsedProperty::Default(100))); // base kept when other is None
        assert_eq!(
            merged.max_concurrent_network,
            Some(ParsedProperty::Default(200))
        );
        assert_eq!(
            merged.max_concurrent_processing,
            Some(ParsedProperty::Default(15))
        );
        assert_eq!(
            merged.tag_prefix,
            Some(ParsedProperty::Default("base-".to_string()))
        ); // base kept when other is None
    }

    /// # Config Merge (Empty Configurations)
    ///
    /// Tests merging behavior with empty configurations.
    ///
    /// ## Test Scenario
    /// - Merges empty configurations together
    /// - Tests edge cases of merging with no values
    ///
    /// ## Expected Outcome
    /// - Empty merges produce empty results
    /// - No errors occur when merging empty configurations
    #[test]
    fn test_config_merge_empty_configs() {
        let empty1 = Config {
            organization: None,
            project: None,
            repository: None,
            pat: None,
            dev_branch: None,
            target_branch: None,
            local_repo: None,
            work_item_state: None,
            parallel_limit: None,
            max_concurrent_network: None,
            max_concurrent_processing: None,
            tag_prefix: None,
            run_hooks: None,
            show_dependency_highlights: None,
            show_work_item_highlights: None,
            repo_aliases: None,
        };

        let empty2 = Config {
            organization: None,
            project: None,
            repository: None,
            pat: None,
            dev_branch: None,
            target_branch: None,
            local_repo: None,
            work_item_state: None,
            parallel_limit: None,
            max_concurrent_network: None,
            max_concurrent_processing: None,
            tag_prefix: None,
            run_hooks: None,
            show_dependency_highlights: None,
            show_work_item_highlights: None,
            repo_aliases: None,
        };

        let merged = empty1.merge(empty2);

        assert_eq!(merged.organization, None);
        assert_eq!(merged.project, None);
        assert_eq!(merged.repository, None);
        assert_eq!(merged.pat, None);
        assert_eq!(merged.dev_branch, None);
        assert_eq!(merged.target_branch, None);
        assert_eq!(merged.local_repo, None);
        assert_eq!(merged.work_item_state, None);
        assert_eq!(merged.parallel_limit, None);
        assert_eq!(merged.max_concurrent_network, None);
        assert_eq!(merged.max_concurrent_processing, None);
        assert_eq!(merged.tag_prefix, None);
    }

    /// # Load Config from File (Valid TOML)
    ///
    /// Tests loading configuration from a valid TOML file.
    ///
    /// ## Test Scenario
    /// - Creates a temporary TOML config file with valid content
    /// - Loads configuration from the file
    ///
    /// ## Expected Outcome
    /// - TOML file is correctly parsed
    /// - All configuration values are properly loaded
    #[test]
    #[file_serial(env_tests)]
    fn test_load_from_file_valid_toml() {
        // Clean up env vars from other tests that could interfere
        unsafe {
            env::remove_var("MERGERS_ORGANIZATION");
            env::remove_var("MERGERS_PROJECT");
            env::remove_var("MERGERS_REPOSITORY");
            env::remove_var("MERGERS_PAT");
        }

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let toml_content = r#"
organization = "file-org"
project = "file-project"
repository = "file-repo"
dev_branch = "file-dev"
target_branch = "file-target"
work_item_state = "File State"
parallel_limit = 250
max_concurrent_network = 150
max_concurrent_processing = 12
tag_prefix = "file-"
"#;

        fs::write(&config_path, toml_content).unwrap();

        // Temporarily override the config path by setting XDG_CONFIG_HOME
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            env::set_var(
                "XDG_CONFIG_HOME",
                temp_dir.path().join("mergers").parent().unwrap(),
            );
        }

        // Create the mergers directory and move our config file there
        let mergers_dir = temp_dir.path().join("mergers");
        fs::create_dir_all(&mergers_dir).unwrap();
        let final_config_path = mergers_dir.join("config.toml");
        fs::write(&final_config_path, toml_content).unwrap();

        let result = Config::load_from_file();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());
        let config = result.unwrap();

        assert_eq!(
            config.organization,
            Some(ParsedProperty::File(
                "file-org".to_string(),
                final_config_path.clone(),
                "file-org".to_string()
            ))
        );
        assert_eq!(
            config.project,
            Some(ParsedProperty::File(
                "file-project".to_string(),
                final_config_path.clone(),
                "file-project".to_string()
            ))
        );
        assert_eq!(
            config.repository,
            Some(ParsedProperty::File(
                "file-repo".to_string(),
                final_config_path.clone(),
                "file-repo".to_string()
            ))
        );
        assert_eq!(
            config.dev_branch,
            Some(ParsedProperty::File(
                "file-dev".to_string(),
                final_config_path.clone(),
                "file-dev".to_string()
            ))
        );
        assert_eq!(
            config.target_branch,
            Some(ParsedProperty::File(
                "file-target".to_string(),
                final_config_path.clone(),
                "file-target".to_string()
            ))
        );
        assert_eq!(
            config.work_item_state,
            Some(ParsedProperty::File(
                "File State".to_string(),
                final_config_path.clone(),
                "File State".to_string()
            ))
        );
        assert_eq!(
            config.parallel_limit,
            Some(ParsedProperty::File(
                250,
                final_config_path.clone(),
                "250".to_string()
            ))
        );
        assert_eq!(
            config.max_concurrent_network,
            Some(ParsedProperty::File(
                150,
                final_config_path.clone(),
                "150".to_string()
            ))
        );
        assert_eq!(
            config.max_concurrent_processing,
            Some(ParsedProperty::File(
                12,
                final_config_path.clone(),
                "12".to_string()
            ))
        );
        assert_eq!(
            config.tag_prefix,
            Some(ParsedProperty::File(
                "file-".to_string(),
                final_config_path.clone(),
                "file-".to_string()
            ))
        );
    }

    /// # Load Config from File (Missing File Returns Default)
    ///
    /// Tests behavior when attempting to load from a non-existent config file.
    ///
    /// ## Test Scenario
    /// - Attempts to load configuration from a missing file
    /// - Tests fallback to default configuration
    ///
    /// ## Expected Outcome
    /// - Missing file doesn't cause errors
    /// - Default configuration is returned when file is missing
    #[test]
    #[file_serial(env_tests)]
    fn test_load_from_file_missing_file_returns_default() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        // Set XDG_CONFIG_HOME to a temp directory where no config exists
        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        let result = Config::load_from_file();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());
        let config = result.unwrap();

        // Should be default config
        let default_config = Config::default();
        assert_eq!(config.dev_branch, default_config.dev_branch);
        assert_eq!(config.target_branch, default_config.target_branch);
        assert_eq!(config.parallel_limit, default_config.parallel_limit);
    }

    /// # Load Config from File (Invalid TOML)
    ///
    /// Tests handling of invalid TOML syntax in configuration files.
    ///
    /// ## Test Scenario
    /// - Creates a config file with invalid TOML syntax
    /// - Attempts to load and parse the malformed file
    ///
    /// ## Expected Outcome
    /// - Invalid TOML is handled gracefully
    /// - Error is returned or default config is used
    #[test]
    #[file_serial(env_tests)]
    fn test_load_from_file_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let mergers_dir = temp_dir.path().join("mergers");
        fs::create_dir_all(&mergers_dir).unwrap();
        let config_path = mergers_dir.join("config.toml");

        // Write invalid TOML
        let invalid_toml = r#"
organization = "test"
invalid toml syntax here [
"#;

        fs::write(&config_path, invalid_toml).unwrap();

        let original_xdg = env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        let result = Config::load_from_file();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_err());
        // Should return error when TOML is invalid
    }

    /// # Detect Config from Git Remote (Error Handling)
    ///
    /// Tests git remote detection when git operations fail.
    ///
    /// ## Test Scenario
    /// - Attempts to detect configuration from git remote in invalid context
    /// - Tests error handling for git command failures
    ///
    /// ## Expected Outcome
    /// - Git errors are handled gracefully
    /// - Default configuration is returned on git operation failure
    #[test]
    fn test_detect_from_git_remote_returns_default_on_error() {
        // Test with a non-existent path
        let config = Config::detect_from_git_remote("/non/existent/path");

        // Should return default config when git detection fails
        let default_config = Config::default();
        assert_eq!(config.dev_branch, default_config.dev_branch);
        assert_eq!(config.target_branch, default_config.target_branch);
        assert_eq!(config.organization, None); // Git detection clears some fields
    }

    /// # Create Sample Config File
    ///
    /// Tests creation of a sample configuration file.
    ///
    /// ## Test Scenario
    /// - Creates a sample configuration file in a temporary directory
    /// - Validates file creation and content structure
    ///
    /// ## Expected Outcome
    /// - Sample config file is successfully created
    /// - File contains expected configuration template
    #[test]
    #[file_serial(env_tests)]
    fn test_create_sample_config_creates_file() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        let result = Config::create_sample_config();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());

        // Check that the config file was created
        let config_path = temp_dir.path().join("mergers").join("config.toml");
        assert!(config_path.exists());

        // Check that it contains sample content
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("# Mergers Configuration File"));
        assert!(content.contains("organization = \"your-organization\""));
        assert!(content.contains("dev_branch = \"dev\""));
    }

    /// # Create Sample Config (No Overwrite)
    ///
    /// Tests that sample config creation doesn't overwrite existing files.
    ///
    /// ## Test Scenario
    /// - Creates an existing config file
    /// - Attempts to create sample config in same location
    ///
    /// ## Expected Outcome
    /// - Existing files are not overwritten
    /// - Safe behavior prevents data loss
    #[test]
    #[file_serial(env_tests)]
    fn test_create_sample_config_does_not_overwrite() {
        let temp_dir = TempDir::new().unwrap();
        let mergers_dir = temp_dir.path().join("mergers");
        fs::create_dir_all(&mergers_dir).unwrap();
        let config_path = mergers_dir.join("config.toml");

        // Create existing config
        fs::write(&config_path, "existing content").unwrap();

        let original_xdg = env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        let result = Config::create_sample_config();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());

        // Check that existing content was not overwritten
        let content = fs::read_to_string(&config_path).unwrap();
        assert_eq!(content, "existing content");
    }

    /// # Get Config Path (XDG Config Home)
    ///
    /// Tests that configuration path respects XDG_CONFIG_HOME environment variable.
    ///
    /// ## Test Scenario
    /// - Sets XDG_CONFIG_HOME environment variable
    /// - Gets configuration file path
    ///
    /// ## Expected Outcome
    /// - Configuration path uses XDG_CONFIG_HOME when set
    /// - Path follows XDG Base Directory specification
    #[test]
    #[file_serial(env_tests)]
    fn test_get_config_path_uses_xdg_config_home() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        let result = Config::get_config_path();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());
        let path = result.unwrap();
        assert_eq!(path, temp_dir.path().join("mergers").join("config.toml"));
    }

    /// # Config Serialization
    ///
    /// Tests serialization and deserialization of configuration objects.
    ///
    /// ## Test Scenario
    /// - Creates a configuration object with various values
    /// - Serializes to TOML and deserializes back
    ///
    /// ## Expected Outcome
    /// - Configuration serializes correctly to TOML
    /// - Deserialized object matches original configuration
    #[test]
    fn test_config_serialization() {
        let config = Config {
            organization: Some(ParsedProperty::Default("test-org".to_string())),
            project: Some(ParsedProperty::Default("test-project".to_string())),
            repository: Some(ParsedProperty::Default("test-repo".to_string())),
            pat: Some(ParsedProperty::Default("test-pat".to_string())),
            dev_branch: Some(ParsedProperty::Default("develop".to_string())),
            target_branch: Some(ParsedProperty::Default("main".to_string())),
            local_repo: Some(ParsedProperty::Default("/tmp/repo".to_string())),
            work_item_state: Some(ParsedProperty::Default("Done".to_string())),
            parallel_limit: Some(ParsedProperty::Default(500)),
            max_concurrent_network: Some(ParsedProperty::Default(200)),
            max_concurrent_processing: Some(ParsedProperty::Default(20)),
            tag_prefix: Some(ParsedProperty::Default("release-".to_string())),
            run_hooks: Some(ParsedProperty::Default(false)),
            show_dependency_highlights: Some(ParsedProperty::Default(true)),
            show_work_item_highlights: Some(ParsedProperty::Default(true)),
            repo_aliases: None,
        };

        // Test serialization to TOML (serializes with enum variant info)
        let toml_string = toml::to_string(&config).unwrap();
        assert!(toml_string.contains("test-org"));
        assert!(toml_string.contains("500"));

        // Test deserialization from TOML
        let deserialized: Config = toml::from_str(&toml_string).unwrap();
        assert_eq!(deserialized.organization, config.organization);
        assert_eq!(deserialized.parallel_limit, config.parallel_limit);
    }

    /// # UI Settings Default Values
    ///
    /// Tests that UI settings have correct default values.
    ///
    /// ## Test Scenario
    /// - Creates a default Config instance
    /// - Validates UI settings fields have expected defaults
    ///
    /// ## Expected Outcome
    /// - show_dependency_highlights defaults to true
    /// - show_work_item_highlights defaults to true
    #[test]
    fn test_ui_settings_defaults() {
        let config = Config::default();

        // Both highlight settings should default to true
        assert_eq!(
            config.show_dependency_highlights,
            Some(ParsedProperty::Default(true))
        );
        assert_eq!(
            config.show_work_item_highlights,
            Some(ParsedProperty::Default(true))
        );
    }

    /// # Save UI Settings Creates Config File
    ///
    /// Tests that save_ui_settings creates a config file with correct values.
    ///
    /// ## Test Scenario
    /// - Sets XDG_CONFIG_HOME to a temp directory
    /// - Saves UI settings with specific values
    /// - Reads back the config file and verifies content
    ///
    /// ## Expected Outcome
    /// - Config file is created in correct location
    /// - UI settings are correctly serialized
    #[test]
    #[file_serial(env_tests)]
    fn test_save_ui_settings_creates_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Save settings with specific values
        let result = Config::save_ui_settings(false, true);

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());

        // Verify the config file was created
        let config_path = temp_dir.path().join("mergers").join("config.toml");
        assert!(config_path.exists());

        // Read and verify content
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("show_dependency_highlights = false"));
        assert!(content.contains("show_work_item_highlights = true"));
    }

    /// # Save UI Settings Preserves Other Settings
    ///
    /// Tests that saving UI settings doesn't overwrite other config values.
    ///
    /// ## Test Scenario
    /// - Creates a config file with existing settings
    /// - Saves UI settings
    /// - Verifies original settings are preserved
    ///
    /// ## Expected Outcome
    /// - Existing settings remain unchanged
    /// - UI settings are added/updated
    #[test]
    #[file_serial(env_tests)]
    fn test_save_ui_settings_preserves_existing_settings() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        // Create the config directory
        let config_dir = temp_dir.path().join("mergers");
        fs::create_dir_all(&config_dir).unwrap();

        // Create an existing config with some settings
        let config_path = config_dir.join("config.toml");
        let existing_content = r#"
organization = "my-org"
project = "my-project"
dev_branch = "develop"
"#;
        fs::write(&config_path, existing_content).unwrap();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Save UI settings
        let result = Config::save_ui_settings(true, false);

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());

        // Read and verify content
        let content = fs::read_to_string(&config_path).unwrap();

        // Original settings should be preserved
        assert!(content.contains("organization"));
        assert!(content.contains("my-org"));
        assert!(content.contains("project"));
        assert!(content.contains("my-project"));
        assert!(content.contains("dev_branch"));
        assert!(content.contains("develop"));

        // UI settings should be present
        assert!(content.contains("show_dependency_highlights = true"));
        assert!(content.contains("show_work_item_highlights = false"));
    }

    /// # Load UI Settings From File
    ///
    /// Tests that UI settings are correctly loaded from config file.
    ///
    /// ## Test Scenario
    /// - Creates a config file with UI settings
    /// - Loads the config
    /// - Verifies UI settings are correctly parsed
    ///
    /// ## Expected Outcome
    /// - UI settings match values in config file
    #[test]
    #[file_serial(env_tests)]
    fn test_load_ui_settings_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        // Create the config directory
        let config_dir = temp_dir.path().join("mergers");
        fs::create_dir_all(&config_dir).unwrap();

        // Create config with UI settings
        let config_path = config_dir.join("config.toml");
        let content = r#"
show_dependency_highlights = false
show_work_item_highlights = true
"#;
        fs::write(&config_path, content).unwrap();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Load config
        let config = Config::load_from_file().unwrap();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        // Verify UI settings
        assert!(config.show_dependency_highlights.is_some());
        assert!(!*config.show_dependency_highlights.unwrap().value());

        assert!(config.show_work_item_highlights.is_some());
        assert!(*config.show_work_item_highlights.unwrap().value());
    }

    /// # UI Settings Round Trip
    ///
    /// Tests saving and loading UI settings preserves values.
    ///
    /// ## Test Scenario
    /// - Saves UI settings with specific values
    /// - Loads the config back
    /// - Verifies values match what was saved
    ///
    /// ## Expected Outcome
    /// - Loaded values match saved values exactly
    #[test]
    #[file_serial(env_tests)]
    fn test_ui_settings_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Save settings
        Config::save_ui_settings(false, false).unwrap();

        // Load and verify
        let config = Config::load_from_file().unwrap();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(!*config.show_dependency_highlights.unwrap().value());
        assert!(!*config.show_work_item_highlights.unwrap().value());
    }

    /// # Config Path Structure
    ///
    /// Tests that the config path follows correct structure.
    ///
    /// ## Test Scenario
    /// - Gets config path with known XDG_CONFIG_HOME
    /// - Verifies path structure
    ///
    /// ## Expected Outcome
    /// - Path is XDG_CONFIG_HOME/mergers/config.toml
    #[test]
    #[file_serial(env_tests)]
    fn test_config_path_structure() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        let path = Config::get_config_path().unwrap();

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        // Verify path structure
        assert!(path.ends_with("config.toml"));
        assert!(path.to_string_lossy().contains("mergers"));
        assert_eq!(path.file_name().unwrap(), "config.toml");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "mergers");
    }

    /// # UI Settings Merge Precedence
    ///
    /// Tests that UI settings follow correct merge precedence.
    ///
    /// ## Test Scenario
    /// - Creates configs with UI settings from different sources
    /// - Merges them in order
    /// - Verifies precedence is correct
    ///
    /// ## Expected Outcome
    /// - Later (higher priority) values override earlier ones
    #[test]
    fn test_ui_settings_merge_precedence() {
        let base = Config {
            organization: None,
            project: None,
            repository: None,
            pat: None,
            dev_branch: None,
            target_branch: None,
            local_repo: None,
            work_item_state: None,
            parallel_limit: None,
            max_concurrent_network: None,
            max_concurrent_processing: None,
            tag_prefix: None,
            run_hooks: None,
            show_dependency_highlights: Some(ParsedProperty::Default(true)),
            show_work_item_highlights: Some(ParsedProperty::Default(true)),
            repo_aliases: None,
        };

        let override_config = Config {
            organization: None,
            project: None,
            repository: None,
            pat: None,
            dev_branch: None,
            target_branch: None,
            local_repo: None,
            work_item_state: None,
            parallel_limit: None,
            max_concurrent_network: None,
            max_concurrent_processing: None,
            tag_prefix: None,
            run_hooks: None,
            show_dependency_highlights: Some(ParsedProperty::Default(false)),
            show_work_item_highlights: None, // Should keep base value
            repo_aliases: None,
        };

        let merged = base.merge(override_config);

        // Override takes precedence
        assert!(!*merged.show_dependency_highlights.unwrap().value());
        // Base value is kept when override is None
        assert!(*merged.show_work_item_highlights.unwrap().value());
    }

    /// # Save UI Settings Creates Directory
    ///
    /// Tests that save_ui_settings creates the config directory if needed.
    ///
    /// ## Test Scenario
    /// - Sets XDG_CONFIG_HOME to a temp directory without mergers subdirectory
    /// - Saves UI settings
    /// - Verifies directory and file are created
    ///
    /// ## Expected Outcome
    /// - mergers directory is created
    /// - config.toml is created inside it
    #[test]
    #[file_serial(env_tests)]
    fn test_save_ui_settings_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let original_xdg = env::var("XDG_CONFIG_HOME").ok();

        // Verify mergers directory doesn't exist yet
        let mergers_dir = temp_dir.path().join("mergers");
        assert!(!mergers_dir.exists());

        unsafe {
            env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Save settings
        let result = Config::save_ui_settings(true, true);

        // Restore original XDG_CONFIG_HOME
        match original_xdg {
            Some(val) => unsafe {
                env::set_var("XDG_CONFIG_HOME", val);
            },
            None => unsafe {
                env::remove_var("XDG_CONFIG_HOME");
            },
        }

        assert!(result.is_ok());

        // Verify directory and file were created
        assert!(mergers_dir.exists());
        assert!(mergers_dir.join("config.toml").exists());
    }

    /// # Resolve Repo Path With Alias
    ///
    /// Tests that resolve_repo_path correctly resolves aliases from config.
    ///
    /// ## Test Scenario
    /// - Creates an alias map with "test" -> "/tmp/test-repo"
    /// - Resolves "test" using the alias map
    ///
    /// ## Expected Outcome
    /// - Path resolves to the alias target
    #[test]
    fn test_resolve_repo_path_with_alias() {
        let mut aliases = HashMap::new();
        aliases.insert("test".to_string(), "/tmp/test-repo".to_string());

        let result = super::resolve_repo_path(Some("test"), &Some(aliases));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from("/tmp/test-repo"));
    }

    /// # Resolve Repo Path Current Directory
    ///
    /// Tests that resolve_repo_path falls back to current directory when no input.
    ///
    /// ## Test Scenario
    /// - Calls resolve_repo_path with None path and None aliases
    ///
    /// ## Expected Outcome
    /// - Returns the current working directory
    #[test]
    fn test_resolve_repo_path_current_dir() {
        let result = super::resolve_repo_path(None, &None);
        assert!(result.is_ok());
    }
}
