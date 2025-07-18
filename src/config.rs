use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            organization: None,
            project: None,
            repository: None,
            pat: None,
            dev_branch: Some("dev".to_string()),
            target_branch: Some("next".to_string()),
            local_repo: None,
            work_item_state: Some("Next Merged".to_string()),
            parallel_limit: Some(300),
            max_concurrent_network: Some(100),
            max_concurrent_processing: Some(10),
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

        let config: Self = toml::from_str(&config_content)
            .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;

        Ok(config)
    }

    /// Load configuration from environment variables
    pub fn load_from_env() -> Self {
        Self {
            organization: std::env::var("MERGERS_ORGANIZATION").ok(),
            project: std::env::var("MERGERS_PROJECT").ok(),
            repository: std::env::var("MERGERS_REPOSITORY").ok(),
            pat: std::env::var("MERGERS_PAT").ok(),
            dev_branch: std::env::var("MERGERS_DEV_BRANCH").ok(),
            target_branch: std::env::var("MERGERS_TARGET_BRANCH").ok(),
            local_repo: std::env::var("MERGERS_LOCAL_REPO").ok(),
            work_item_state: std::env::var("MERGERS_WORK_ITEM_STATE").ok(),
            parallel_limit: std::env::var("MERGERS_PARALLEL_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok()),
            max_concurrent_network: std::env::var("MERGERS_MAX_CONCURRENT_NETWORK")
                .ok()
                .and_then(|s| s.parse().ok()),
            max_concurrent_processing: std::env::var("MERGERS_MAX_CONCURRENT_PROCESSING")
                .ok()
                .and_then(|s| s.parse().ok()),
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
