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

use crate::{git_config, parsed_property::ParsedProperty};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Temporary struct for deserializing TOML configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone)]
pub struct Config {
    pub organization: Option<ParsedProperty<String>>,
    pub project: Option<ParsedProperty<String>>,
    pub repository: Option<ParsedProperty<String>>,
    pub pat: Option<ParsedProperty<String>>,
    pub dev_branch: Option<ParsedProperty<String>>,
    pub target_branch: Option<ParsedProperty<String>>,
    pub local_repo: Option<ParsedProperty<String>>,
    pub work_item_state: Option<ParsedProperty<String>>,
    pub parallel_limit: Option<ParsedProperty<usize>>,
    pub max_concurrent_network: Option<ParsedProperty<usize>>,
    pub max_concurrent_processing: Option<ParsedProperty<usize>>,
    pub tag_prefix: Option<ParsedProperty<String>>,
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
        }
    }
}

impl Config {
    /// Load configuration from XDG config directory
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
        })
    }

    /// Detect configuration from git remote if repository is Azure DevOps
    pub fn detect_from_git_remote<P: AsRef<std::path::Path>>(repo_path: P) -> Self {
        // First try to get the remote URL for better error context
        let remote_url = std::process::Command::new("git")
            .current_dir(repo_path.as_ref())
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

        match git_config::detect_azure_devops_config(repo_path) {
            Ok(Some(azure_config)) => Self {
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
            },
            _ => Self::default(),
        }
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
        }
    }

    /// Create a sample config file for user reference
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_load_from_env_no_variables() {
        // Ensure no relevant env vars are set
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
    fn test_load_from_file_valid_toml() {
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

    // /// # Config Serialization
    // ///
    // /// Tests serialization and deserialization of configuration objects.
    // ///
    // /// ## Test Scenario
    // /// - Creates a configuration object with various values
    // /// - Serializes to TOML and deserializes back
    // ///
    // /// ## Expected Outcome
    // /// - Configuration serializes correctly to TOML
    // /// - Deserialized object matches original configuration
    // #[test]
    // fn test_config_serialization() {
    //     // Note: Disabled because Config contains ParsedProperty which doesn't implement Serialize/Deserialize
    //     let config = Config {
    //         organization: Some(ParsedProperty::Default("test-org".to_string())),
    //         project: Some(ParsedProperty::Default("test-project".to_string())),
    //         repository: Some(ParsedProperty::Default("test-repo".to_string())),
    //         pat: Some(ParsedProperty::Default("test-pat".to_string())),
    //         dev_branch: Some(ParsedProperty::Default("develop".to_string())),
    //         target_branch: Some(ParsedProperty::Default("main".to_string())),
    //         local_repo: Some(ParsedProperty::Default("/tmp/repo".to_string())),
    //         work_item_state: Some(ParsedProperty::Default("Done".to_string())),
    //         parallel_limit: Some(ParsedProperty::Default(500)),
    //         max_concurrent_network: Some(ParsedProperty::Default(200)),
    //         max_concurrent_processing: Some(ParsedProperty::Default(20)),
    //         tag_prefix: Some(ParsedProperty::Default("release-".to_string())),
    //     };

    //     // Test serialization to TOML
    //     let toml_string = toml::to_string(&config).unwrap();
    //     assert!(toml_string.contains("organization = \"test-org\""));
    //     assert!(toml_string.contains("parallel_limit = 500"));

    //     // Test deserialization from TOML
    //     let deserialized: Config = toml::from_str(&toml_string).unwrap();
    //     assert_eq!(deserialized.organization, config.organization);
    //     assert_eq!(deserialized.parallel_limit, config.parallel_limit);
    // }
}
