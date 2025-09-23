use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct AzureDevOpsConfig {
    pub organization: String,
    pub project: String,
    pub repository: String,
}

// Static regex patterns compiled once using OnceLock
static SSH_LEGACY_REGEX: OnceLock<Regex> = OnceLock::new();
static SSH_MODERN_REGEX: OnceLock<Regex> = OnceLock::new();
static HTTPS_GIT_REGEX: OnceLock<Regex> = OnceLock::new();
static HTTPS_SIMPLE_REGEX: OnceLock<Regex> = OnceLock::new();
static LEGACY_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_ssh_legacy_regex() -> &'static Regex {
    SSH_LEGACY_REGEX.get_or_init(|| {
        Regex::new(r"^([^@]+)@vs-ssh\.visualstudio\.com:v3/([^/]+)/([^/]+)/([^/]+)/?$")
            .expect("Failed to compile SSH legacy regex")
    })
}

fn get_ssh_modern_regex() -> &'static Regex {
    SSH_MODERN_REGEX.get_or_init(|| {
        Regex::new(r"^([^@]+)@ssh\.dev\.azure\.com:v3/([^/]+)/([^/]+)/([^/]+)/?$")
            .expect("Failed to compile SSH modern regex")
    })
}

fn get_https_git_regex() -> &'static Regex {
    HTTPS_GIT_REGEX.get_or_init(|| {
        Regex::new(r"^https://[^@]*@?dev\.azure\.com/([^/]+)/([^/]+)/_git/([^/]+)/?$")
            .expect("Failed to compile HTTPS _git regex")
    })
}

fn get_https_simple_regex() -> &'static Regex {
    HTTPS_SIMPLE_REGEX.get_or_init(|| {
        Regex::new(r"^https://[^@]*@?dev\.azure\.com/([^/]+)/([^/]+)/([^/]+)/?$")
            .expect("Failed to compile HTTPS simple regex")
    })
}

fn get_legacy_regex() -> &'static Regex {
    LEGACY_REGEX.get_or_init(|| {
        Regex::new(r"^https://([^.]+)\.visualstudio\.com/([^/]+)/_git/([^/]+)/?$")
            .expect("Failed to compile legacy regex")
    })
}

/// Extract Azure DevOps configuration from a git repository's remote URL
pub fn detect_azure_devops_config<P: AsRef<Path>>(
    repo_path: P,
) -> Result<Option<AzureDevOpsConfig>> {
    let repo_path = repo_path.as_ref();

    // Verify this is a git repository
    if !is_git_repository(repo_path)? {
        return Ok(None);
    }

    // Get the remote URL
    let remote_url = get_git_remote_url(repo_path)?;

    // Parse Azure DevOps configuration from the URL
    parse_azure_devops_url(&remote_url)
}

/// Check if the given path is a git repository
fn is_git_repository<P: AsRef<Path>>(repo_path: P) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo_path.as_ref())
        .args(["rev-parse", "--git-dir"])
        .output()
        .context("Failed to check if directory is a git repository")?;

    Ok(output.status.success())
}

/// Get the origin remote URL from a git repository
fn get_git_remote_url<P: AsRef<Path>>(repo_path: P) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_path.as_ref())
        .args(["remote", "get-url", "origin"])
        .output()
        .context("Failed to get git remote URL")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get git remote URL: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Parse Azure DevOps configuration from various URL formats
fn parse_azure_devops_url(url: &str) -> Result<Option<AzureDevOpsConfig>> {
    // Try SSH formats first (most common)
    if let Some(captures) = get_ssh_legacy_regex().captures(url) {
        let organization = captures.get(2).unwrap().as_str().to_string();
        let project = captures.get(3).unwrap().as_str().to_string();
        let repository = captures.get(4).unwrap().as_str().to_string();

        return Ok(Some(AzureDevOpsConfig {
            organization,
            project,
            repository,
        }));
    }

    if let Some(captures) = get_ssh_modern_regex().captures(url) {
        let organization = captures.get(2).unwrap().as_str().to_string();
        let project = captures.get(3).unwrap().as_str().to_string();
        let repository = captures.get(4).unwrap().as_str().to_string();

        return Ok(Some(AzureDevOpsConfig {
            organization,
            project,
            repository,
        }));
    }

    // Try HTTPS format with _git
    if let Some(captures) = get_https_git_regex().captures(url) {
        let organization = captures.get(1).unwrap().as_str().to_string();
        let project = captures.get(2).unwrap().as_str().to_string();
        let repository = captures.get(3).unwrap().as_str().to_string();

        return Ok(Some(AzureDevOpsConfig {
            organization,
            project,
            repository,
        }));
    }

    // Try simple HTTPS format without _git
    if let Some(captures) = get_https_simple_regex().captures(url) {
        let organization = captures.get(1).unwrap().as_str().to_string();
        let project = captures.get(2).unwrap().as_str().to_string();
        let repository = captures.get(3).unwrap().as_str().to_string();

        return Ok(Some(AzureDevOpsConfig {
            organization,
            project,
            repository,
        }));
    }

    // Try legacy HTTPS format
    if let Some(captures) = get_legacy_regex().captures(url) {
        let organization = captures.get(1).unwrap().as_str().to_string();
        let project = captures.get(2).unwrap().as_str().to_string();
        let repository = captures.get(3).unwrap().as_str().to_string();

        return Ok(Some(AzureDevOpsConfig {
            organization,
            project,
            repository,
        }));
    }

    // Not an Azure DevOps URL
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Parse Azure DevOps SSH URL (Legacy Format)
    ///
    /// Tests parsing of legacy Azure DevOps SSH URLs.
    ///
    /// ## Test Scenario
    /// - Provides legacy format SSH URL from Visual Studio Online
    /// - Parses organization, project, and repository information
    ///
    /// ## Expected Outcome
    /// - Legacy SSH URL format is correctly parsed
    /// - Organization, project, and repository are extracted accurately
    #[test]
    fn test_parse_azure_devops_ssh_url_legacy() {
        let url = "ceibaeclinics@vs-ssh.visualstudio.com:v3/ceibaeclinics/EclinicsFrontend/EclinicsFrontend";
        let config = parse_azure_devops_url(url).unwrap().unwrap();

        assert_eq!(config.organization, "ceibaeclinics");
        assert_eq!(config.project, "EclinicsFrontend");
        assert_eq!(config.repository, "EclinicsFrontend");
    }

    /// # Parse Azure DevOps SSH URL (Modern Format)
    ///
    /// Tests parsing of modern Azure DevOps SSH URLs.
    ///
    /// ## Test Scenario
    /// - Provides modern format SSH URL from dev.azure.com
    /// - Parses organization, project, and repository information
    ///
    /// ## Expected Outcome
    /// - Modern SSH URL format is correctly parsed
    /// - All URL components are extracted with proper structure
    #[test]
    fn test_parse_azure_devops_ssh_url_modern() {
        let url = "git@ssh.dev.azure.com:v3/myorg/myproject/myrepo";
        let config = parse_azure_devops_url(url).unwrap().unwrap();

        assert_eq!(config.organization, "myorg");
        assert_eq!(config.project, "myproject");
        assert_eq!(config.repository, "myrepo");
    }

    /// # Parse Azure DevOps HTTPS URL with Git Path
    ///
    /// Tests parsing of HTTPS URLs that include the _git path component.
    ///
    /// ## Test Scenario
    /// - Provides HTTPS URL with _git path and optional user prefix
    /// - Tests extraction from git clone URLs
    ///
    /// ## Expected Outcome
    /// - HTTPS URLs with _git path are correctly parsed
    /// - User prefixes are handled and stripped appropriately
    #[test]
    fn test_parse_azure_devops_https_url_with_git() {
        let url = "https://myorg@dev.azure.com/myorg/myproject/_git/myrepo";
        let config = parse_azure_devops_url(url).unwrap().unwrap();

        assert_eq!(config.organization, "myorg");
        assert_eq!(config.project, "myproject");
        assert_eq!(config.repository, "myrepo");
    }

    /// # Parse Azure DevOps HTTPS URL (Simple Format)
    ///
    /// Tests parsing of simple HTTPS URLs without _git path.
    ///
    /// ## Test Scenario
    /// - Provides simple HTTPS URL format from Azure DevOps
    /// - Tests direct repository URL parsing
    ///
    /// ## Expected Outcome
    /// - Simple HTTPS URLs are correctly parsed
    /// - Repository information is extracted without _git path
    #[test]
    fn test_parse_azure_devops_https_url_simple() {
        let url = "https://dev.azure.com/myorg/myproject/myrepo";
        let config = parse_azure_devops_url(url).unwrap().unwrap();

        assert_eq!(config.organization, "myorg");
        assert_eq!(config.project, "myproject");
        assert_eq!(config.repository, "myrepo");
    }

    /// # Parse Azure DevOps HTTPS URL with User (Simple)
    ///
    /// Tests parsing of simple HTTPS URLs that include user credentials.
    ///
    /// ## Test Scenario
    /// - Provides HTTPS URL with user@ prefix in simple format
    /// - Tests user credential handling in URL parsing
    ///
    /// ## Expected Outcome
    /// - User credentials are properly handled and stripped
    /// - Repository information is extracted correctly
    #[test]
    fn test_parse_azure_devops_https_url_with_user_simple() {
        let url = "https://user@dev.azure.com/myorg/myproject/myrepo";
        let config = parse_azure_devops_url(url).unwrap().unwrap();

        assert_eq!(config.organization, "myorg");
        assert_eq!(config.project, "myproject");
        assert_eq!(config.repository, "myrepo");
    }

    /// # Parse Azure DevOps Legacy URL
    ///
    /// Tests parsing of legacy visualstudio.com domain URLs.
    ///
    /// ## Test Scenario
    /// - Provides legacy Visual Studio Team Services URL format
    /// - Tests backward compatibility with old domain structure
    ///
    /// ## Expected Outcome
    /// - Legacy domain URLs are correctly parsed
    /// - Organization and project information is properly extracted
    #[test]
    fn test_parse_azure_devops_legacy_url() {
        let url = "https://myorg.visualstudio.com/myproject/_git/myrepo";
        let config = parse_azure_devops_url(url).unwrap().unwrap();

        assert_eq!(config.organization, "myorg");
        assert_eq!(config.project, "myproject");
        assert_eq!(config.repository, "myrepo");
    }

    /// # Parse Non-Azure DevOps URL
    ///
    /// Tests parsing behavior for URLs that are not Azure DevOps repositories.
    ///
    /// ## Test Scenario
    /// - Provides GitHub or other Git hosting service URLs
    /// - Tests that non-Azure DevOps URLs are properly rejected
    ///
    /// ## Expected Outcome
    /// - Non-Azure DevOps URLs return None or appropriate error
    /// - Parser correctly identifies incompatible URL formats
    #[test]
    fn test_parse_non_azure_devops_url() {
        let url = "git@github.com:user/repo.git";
        let config = parse_azure_devops_url(url).unwrap();

        assert!(config.is_none());
    }

    /// # Parse Azure DevOps URL with Trailing Slash
    ///
    /// Tests parsing of URLs with trailing slash characters.
    ///
    /// ## Test Scenario
    /// - Provides Azure DevOps URLs with trailing slashes
    /// - Tests URL normalization and cleanup
    ///
    /// ## Expected Outcome
    /// - Trailing slashes are properly handled and normalized
    /// - Repository information is extracted despite formatting
    #[test]
    fn test_parse_azure_devops_url_with_trailing_slash() {
        let url = "ceibaeclinics@vs-ssh.visualstudio.com:v3/ceibaeclinics/EclinicsFrontend/EclinicsFrontend/";
        let config = parse_azure_devops_url(url).unwrap().unwrap();

        assert_eq!(config.organization, "ceibaeclinics");
        assert_eq!(config.project, "EclinicsFrontend");
        assert_eq!(config.repository, "EclinicsFrontend");
    }
}
