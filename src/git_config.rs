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

/// Generic Git configuration extracted from non-Azure DevOps URLs (GitHub, GitLab, etc.)
#[derive(Debug, Clone)]
pub struct GenericGitConfig {
    /// The owner/organization (e.g., "egeapak" from github.com/egeapak/mergers)
    pub owner: String,
    /// The repository name (e.g., "mergers" from github.com/egeapak/mergers)
    pub repository: String,
}

// Static regex patterns compiled once using OnceLock
static SSH_LEGACY_REGEX: OnceLock<Regex> = OnceLock::new();
static SSH_MODERN_REGEX: OnceLock<Regex> = OnceLock::new();
static HTTPS_GIT_REGEX: OnceLock<Regex> = OnceLock::new();
static HTTPS_SIMPLE_REGEX: OnceLock<Regex> = OnceLock::new();
static LEGACY_REGEX: OnceLock<Regex> = OnceLock::new();
// GitHub patterns
static GITHUB_SSH_REGEX: OnceLock<Regex> = OnceLock::new();
static GITHUB_HTTPS_REGEX: OnceLock<Regex> = OnceLock::new();
// Generic git patterns (GitLab, Bitbucket, etc.)
static GENERIC_SSH_REGEX: OnceLock<Regex> = OnceLock::new();
static GENERIC_HTTPS_REGEX: OnceLock<Regex> = OnceLock::new();

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

fn get_github_ssh_regex() -> &'static Regex {
    GITHUB_SSH_REGEX.get_or_init(|| {
        // Matches: git@github.com:owner/repo.git or git@github.com:owner/repo
        Regex::new(r"^[^@]+@github\.com:([^/]+)/([^/]+?)(?:\.git)?/?$")
            .expect("Failed to compile GitHub SSH regex")
    })
}

fn get_github_https_regex() -> &'static Regex {
    GITHUB_HTTPS_REGEX.get_or_init(|| {
        // Matches: https://github.com/owner/repo.git or https://github.com/owner/repo
        Regex::new(r"^https://(?:[^@]+@)?github\.com/([^/]+)/([^/]+?)(?:\.git)?/?$")
            .expect("Failed to compile GitHub HTTPS regex")
    })
}

fn get_generic_ssh_regex() -> &'static Regex {
    GENERIC_SSH_REGEX.get_or_init(|| {
        // Matches: git@<host>:<owner>/<repo>.git (generic SSH format)
        Regex::new(r"^[^@]+@[^:]+:([^/]+)/([^/]+?)(?:\.git)?/?$")
            .expect("Failed to compile generic SSH regex")
    })
}

fn get_generic_https_regex() -> &'static Regex {
    GENERIC_HTTPS_REGEX.get_or_init(|| {
        // Matches: https://<host>/<owner>/<repo>.git (generic HTTPS format)
        Regex::new(r"^https://(?:[^@]+@)?[^/]+/([^/]+)/([^/]+?)(?:\.git)?/?$")
            .expect("Failed to compile generic HTTPS regex")
    })
}

/// Extract Azure DevOps configuration from regex captures.
///
/// This helper function reduces code duplication by extracting organization,
/// project, and repository from regex captures using the specified indices.
fn extract_config_from_captures(
    captures: &regex::Captures,
    org_idx: usize,
    proj_idx: usize,
    repo_idx: usize,
) -> AzureDevOpsConfig {
    AzureDevOpsConfig {
        organization: captures.get(org_idx).unwrap().as_str().to_string(),
        project: captures.get(proj_idx).unwrap().as_str().to_string(),
        repository: captures.get(repo_idx).unwrap().as_str().to_string(),
    }
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
    // Try SSH formats first (most common) - org/project/repo at indices 2/3/4
    if let Some(captures) = get_ssh_legacy_regex().captures(url) {
        return Ok(Some(extract_config_from_captures(&captures, 2, 3, 4)));
    }

    if let Some(captures) = get_ssh_modern_regex().captures(url) {
        return Ok(Some(extract_config_from_captures(&captures, 2, 3, 4)));
    }

    // Try HTTPS formats - org/project/repo at indices 1/2/3
    if let Some(captures) = get_https_git_regex().captures(url) {
        return Ok(Some(extract_config_from_captures(&captures, 1, 2, 3)));
    }

    if let Some(captures) = get_https_simple_regex().captures(url) {
        return Ok(Some(extract_config_from_captures(&captures, 1, 2, 3)));
    }

    if let Some(captures) = get_legacy_regex().captures(url) {
        return Ok(Some(extract_config_from_captures(&captures, 1, 2, 3)));
    }

    // Not an Azure DevOps URL
    Ok(None)
}

/// Extract generic Git configuration from a repository's remote URL.
///
/// This function extracts owner and repository name from GitHub, GitLab,
/// Bitbucket, and other standard Git hosting URLs.
pub fn detect_generic_git_config<P: AsRef<Path>>(repo_path: P) -> Result<Option<GenericGitConfig>> {
    let repo_path = repo_path.as_ref();

    // Verify this is a git repository
    if !is_git_repository(repo_path)? {
        return Ok(None);
    }

    // Get the remote URL
    let remote_url = get_git_remote_url(repo_path)?;

    // Parse generic Git configuration from the URL
    parse_generic_git_url(&remote_url)
}

/// Parse generic Git configuration from various URL formats (GitHub, GitLab, etc.)
pub fn parse_generic_git_url(url: &str) -> Result<Option<GenericGitConfig>> {
    // Try GitHub SSH format: git@github.com:owner/repo.git
    if let Some(captures) = get_github_ssh_regex().captures(url) {
        return Ok(Some(GenericGitConfig {
            owner: captures.get(1).unwrap().as_str().to_string(),
            repository: captures.get(2).unwrap().as_str().to_string(),
        }));
    }

    // Try GitHub HTTPS format: https://github.com/owner/repo.git
    if let Some(captures) = get_github_https_regex().captures(url) {
        return Ok(Some(GenericGitConfig {
            owner: captures.get(1).unwrap().as_str().to_string(),
            repository: captures.get(2).unwrap().as_str().to_string(),
        }));
    }

    // Try generic SSH format: git@host:owner/repo.git
    if let Some(captures) = get_generic_ssh_regex().captures(url) {
        return Ok(Some(GenericGitConfig {
            owner: captures.get(1).unwrap().as_str().to_string(),
            repository: captures.get(2).unwrap().as_str().to_string(),
        }));
    }

    // Try generic HTTPS format: https://host/owner/repo.git
    if let Some(captures) = get_generic_https_regex().captures(url) {
        return Ok(Some(GenericGitConfig {
            owner: captures.get(1).unwrap().as_str().to_string(),
            repository: captures.get(2).unwrap().as_str().to_string(),
        }));
    }

    // Could not parse the URL
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

    // ============================================================================
    // Generic Git URL Parsing Tests
    // ============================================================================

    /// # Parse GitHub SSH URL
    ///
    /// Tests parsing of GitHub SSH URLs.
    ///
    /// ## Test Scenario
    /// - Provides GitHub SSH URL format
    /// - Parses owner and repository information
    ///
    /// ## Expected Outcome
    /// - GitHub SSH URL is correctly parsed
    /// - Owner and repository are extracted accurately
    #[test]
    fn test_parse_github_ssh_url() {
        let url = "git@github.com:egeapak/mergers.git";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "egeapak");
        assert_eq!(config.repository, "mergers");
    }

    /// # Parse GitHub SSH URL Without .git Extension
    ///
    /// Tests parsing of GitHub SSH URLs without .git suffix.
    ///
    /// ## Test Scenario
    /// - Provides GitHub SSH URL without .git extension
    ///
    /// ## Expected Outcome
    /// - URL is correctly parsed even without .git suffix
    #[test]
    fn test_parse_github_ssh_url_no_git_extension() {
        let url = "git@github.com:user/repo";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "user");
        assert_eq!(config.repository, "repo");
    }

    /// # Parse GitHub HTTPS URL
    ///
    /// Tests parsing of GitHub HTTPS URLs.
    ///
    /// ## Test Scenario
    /// - Provides GitHub HTTPS URL format
    /// - Parses owner and repository information
    ///
    /// ## Expected Outcome
    /// - GitHub HTTPS URL is correctly parsed
    /// - Owner and repository are extracted accurately
    #[test]
    fn test_parse_github_https_url() {
        let url = "https://github.com/egeapak/mergers.git";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "egeapak");
        assert_eq!(config.repository, "mergers");
    }

    /// # Parse GitHub HTTPS URL Without .git Extension
    ///
    /// Tests parsing of GitHub HTTPS URLs without .git suffix.
    ///
    /// ## Test Scenario
    /// - Provides GitHub HTTPS URL without .git extension
    ///
    /// ## Expected Outcome
    /// - URL is correctly parsed even without .git suffix
    #[test]
    fn test_parse_github_https_url_no_git_extension() {
        let url = "https://github.com/user/repo";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "user");
        assert_eq!(config.repository, "repo");
    }

    /// # Parse GitHub HTTPS URL with User Credentials
    ///
    /// Tests parsing of GitHub HTTPS URLs with embedded credentials.
    ///
    /// ## Test Scenario
    /// - Provides GitHub HTTPS URL with user@domain format
    ///
    /// ## Expected Outcome
    /// - URL is correctly parsed, credentials stripped
    #[test]
    fn test_parse_github_https_url_with_credentials() {
        let url = "https://user@github.com/owner/repo.git";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "owner");
        assert_eq!(config.repository, "repo");
    }

    /// # Parse GitLab SSH URL
    ///
    /// Tests parsing of GitLab SSH URLs using generic pattern.
    ///
    /// ## Test Scenario
    /// - Provides GitLab SSH URL format
    ///
    /// ## Expected Outcome
    /// - GitLab SSH URL is correctly parsed using generic pattern
    #[test]
    fn test_parse_gitlab_ssh_url() {
        let url = "git@gitlab.com:company/project.git";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "company");
        assert_eq!(config.repository, "project");
    }

    /// # Parse GitLab HTTPS URL
    ///
    /// Tests parsing of GitLab HTTPS URLs using generic pattern.
    ///
    /// ## Test Scenario
    /// - Provides GitLab HTTPS URL format
    ///
    /// ## Expected Outcome
    /// - GitLab HTTPS URL is correctly parsed using generic pattern
    #[test]
    fn test_parse_gitlab_https_url() {
        let url = "https://gitlab.com/company/project.git";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "company");
        assert_eq!(config.repository, "project");
    }

    /// # Parse Bitbucket SSH URL
    ///
    /// Tests parsing of Bitbucket SSH URLs using generic pattern.
    ///
    /// ## Test Scenario
    /// - Provides Bitbucket SSH URL format
    ///
    /// ## Expected Outcome
    /// - Bitbucket SSH URL is correctly parsed
    #[test]
    fn test_parse_bitbucket_ssh_url() {
        let url = "git@bitbucket.org:team/repository.git";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "team");
        assert_eq!(config.repository, "repository");
    }

    /// # Parse Self-Hosted Git URL
    ///
    /// Tests parsing of self-hosted Git server URLs.
    ///
    /// ## Test Scenario
    /// - Provides self-hosted Git server URL
    ///
    /// ## Expected Outcome
    /// - Self-hosted URLs are correctly parsed using generic pattern
    #[test]
    fn test_parse_self_hosted_git_url() {
        let url = "https://git.company.com/team/internal-repo.git";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "team");
        assert_eq!(config.repository, "internal-repo");
    }

    /// # Parse URL with Trailing Slash
    ///
    /// Tests parsing of Git URLs with trailing slashes.
    ///
    /// ## Test Scenario
    /// - Provides GitHub URL with trailing slash
    ///
    /// ## Expected Outcome
    /// - Trailing slashes are properly handled
    #[test]
    fn test_parse_git_url_with_trailing_slash() {
        let url = "https://github.com/user/repo/";
        let config = parse_generic_git_url(url).unwrap().unwrap();

        assert_eq!(config.owner, "user");
        assert_eq!(config.repository, "repo");
    }
}
