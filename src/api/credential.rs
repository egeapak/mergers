//! PAT-based credential adapter for Azure DevOps API.
//!
//! This module provides a credential implementation that uses Personal Access Tokens (PAT)
//! for authentication with Azure DevOps.

use azure_core::credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions};
use secrecy::{ExposeSecret, SecretString};

/// PAT-based credential for Azure DevOps authentication.
///
/// This credential type wraps a Personal Access Token and presents it as a bearer token
/// for Azure DevOps API requests. The PAT is stored securely using `SecretString`.
///
/// # Example
///
/// ```rust,no_run
/// use mergers::api::PatCredential;
/// use secrecy::SecretString;
/// use std::sync::Arc;
///
/// let pat = SecretString::from("your-pat-token".to_string());
/// let credential = Arc::new(PatCredential::new(pat));
/// ```
#[derive(Clone)]
pub struct PatCredential {
    pat: SecretString,
}

impl PatCredential {
    /// Creates a new PAT credential from a SecretString.
    ///
    /// # Arguments
    ///
    /// * `pat` - The Personal Access Token wrapped in a SecretString
    pub fn new(pat: SecretString) -> Self {
        Self { pat }
    }

    /// Creates a new PAT credential from a plain string.
    ///
    /// The string will be wrapped in a SecretString for secure handling.
    ///
    /// # Arguments
    ///
    /// * `pat` - The Personal Access Token as a plain string
    pub fn from_string(pat: String) -> Self {
        Self {
            pat: SecretString::from(pat),
        }
    }
}

impl std::fmt::Debug for PatCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatCredential")
            .field("pat", &"[REDACTED]")
            .finish()
    }
}

#[async_trait::async_trait]
impl TokenCredential for PatCredential {
    /// Returns the PAT as an access token.
    ///
    /// Azure DevOps uses Basic authentication with the PAT, but the azure_devops_rust_api
    /// crate handles the encoding internally. We return the raw PAT here.
    async fn get_token(
        &self,
        _scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::error::Result<AccessToken> {
        // The azure_devops_rust_api crate expects the raw PAT - it handles
        // the Basic auth encoding internally
        Ok(AccessToken::new(
            Secret::new(self.pat.expose_secret().to_string()),
            // Set a far-future expiry since PATs don't expire through OAuth flow
            time::OffsetDateTime::now_utc() + time::Duration::days(365),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # PatCredential Creation
    ///
    /// Tests that PatCredential can be created from a SecretString.
    ///
    /// ## Test Scenario
    /// - Creates a PatCredential from a SecretString
    /// - Verifies the credential is created successfully
    ///
    /// ## Expected Outcome
    /// - Credential is created without errors
    #[test]
    fn test_pat_credential_creation() {
        let pat = SecretString::from("test-pat".to_string());
        let credential = PatCredential::new(pat);
        // Just verify it compiles and creates
        assert!(format!("{:?}", credential).contains("[REDACTED]"));
    }

    /// # PatCredential from String
    ///
    /// Tests that PatCredential can be created from a plain string.
    ///
    /// ## Test Scenario
    /// - Creates a PatCredential from a plain string
    /// - Verifies the credential is created successfully
    ///
    /// ## Expected Outcome
    /// - Credential is created and PAT is wrapped securely
    #[test]
    fn test_pat_credential_from_string() {
        let credential = PatCredential::from_string("test-pat".to_string());
        assert!(format!("{:?}", credential).contains("[REDACTED]"));
    }

    /// # Token Retrieval
    ///
    /// Tests that the credential returns a valid access token.
    ///
    /// ## Test Scenario
    /// - Creates a PatCredential
    /// - Requests a token from the credential
    ///
    /// ## Expected Outcome
    /// - Token is returned successfully
    /// - Token contains the PAT value
    #[tokio::test]
    async fn test_get_token() {
        let pat = SecretString::from("test-pat-value".to_string());
        let credential = PatCredential::new(pat);

        let token = credential.get_token(&[], None).await.unwrap();
        assert_eq!(token.token.secret(), "test-pat-value");
    }
}
