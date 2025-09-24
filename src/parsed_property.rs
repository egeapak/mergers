use std::{fmt::Display, ops::Deref, path::PathBuf};

/// A configuration property that tracks its source and original value
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ParsedProperty<T> {
    /// Value from command line arguments (parsed_value, original_string)
    Cli(T, String),
    /// Value from environment variable (parsed_value, env_var_value)
    Env(T, String),
    /// Value auto-detected from git remote (parsed_value, git_url)
    Git(T, String),
    /// Value from configuration file (parsed_value, toml_value_string)
    File(T, PathBuf, String),
    /// Default value when no other source provided
    Default(T),
}

impl<T> ParsedProperty<T> {
    /// Get the parsed value
    pub fn value(&self) -> &T {
        match self {
            ParsedProperty::Cli(value, _) => value,
            ParsedProperty::Env(value, _) => value,
            ParsedProperty::Git(value, _) => value,
            ParsedProperty::File(value, _, _) => value,
            ParsedProperty::Default(value) => value,
        }
    }

    /// Get the source name as a string
    pub fn source_name(&self) -> &'static str {
        match self {
            ParsedProperty::Cli(_, _) => "cli",
            ParsedProperty::Env(_, _) => "env",
            ParsedProperty::Git(_, _) => "git",
            ParsedProperty::File(_, _, _) => "file",
            ParsedProperty::Default(_) => "default",
        }
    }

    /// Get the original string value if available
    pub fn original(&self) -> Option<&str> {
        match self {
            ParsedProperty::Cli(_, original) => Some(original),
            ParsedProperty::Env(_, original) => Some(original),
            ParsedProperty::Git(_, original) => Some(original),
            ParsedProperty::File(_, _, original) => Some(original),
            ParsedProperty::Default(_) => None,
        }
    }

    /// Check if this property came from a specific source
    pub fn is_from_source(&self, source: &str) -> bool {
        self.source_name() == source
    }
}

impl<T> Deref for ParsedProperty<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

impl<T: Display> Display for ParsedProperty<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value().fmt(f)
    }
}

impl<T: AsRef<str>> AsRef<str> for ParsedProperty<T> {
    fn as_ref(&self) -> &str {
        self.value().as_ref()
    }
}

impl<T> From<T> for ParsedProperty<T> {
    fn from(value: T) -> Self {
        ParsedProperty::Default(value)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// # ParsedProperty Value Access
    ///
    /// Tests accessing the parsed value from different source variants.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty instances from different sources
    /// - Tests value access via value() method and Deref
    ///
    /// ## Expected Outcome
    /// - Both value() and deref return the same parsed value
    /// - Source information is preserved correctly
    #[test]
    fn test_parsed_property_value_access() {
        let cli_prop = ParsedProperty::Cli(
            "test-org".to_string(),
            "--organization test-org".to_string(),
        );
        let env_prop = ParsedProperty::Env("test-org".to_string(), "test-org".to_string());
        let git_prop = ParsedProperty::Git(
            "test-org".to_string(),
            "https://dev.azure.com/test-org/...".to_string(),
        );
        let file_prop = ParsedProperty::File(
            "test-org".to_string(),
            PathBuf::from("path/to/file"),
            "organization = \"test-org\"".to_string(),
        );
        let default_prop = ParsedProperty::Default("test-org".to_string());

        // Test value() method
        assert_eq!(cli_prop.value(), "test-org");
        assert_eq!(env_prop.value(), "test-org");
        assert_eq!(git_prop.value(), "test-org");
        assert_eq!(file_prop.value(), "test-org");
        assert_eq!(default_prop.value(), "test-org");

        // Test Deref
        assert_eq!(&*cli_prop, "test-org");
        assert_eq!(&*env_prop, "test-org");
        assert_eq!(&*git_prop, "test-org");
        assert_eq!(&*file_prop, "test-org");
        assert_eq!(&*default_prop, "test-org");
    }

    /// # ParsedProperty Source Tracking
    ///
    /// Tests source name and original value tracking.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty instances from different sources
    /// - Tests source_name() and original() methods
    ///
    /// ## Expected Outcome
    /// - Source names are correctly identified
    /// - Original values are preserved for non-default sources
    #[test]
    fn test_parsed_property_source_tracking() {
        let cli_prop = ParsedProperty::Cli(
            "test-org".to_string(),
            "--organization test-org".to_string(),
        );
        let env_prop = ParsedProperty::Env(
            "test-org".to_string(),
            "MERGERS_ORGANIZATION=test-org".to_string(),
        );
        let git_prop = ParsedProperty::Git(
            "test-org".to_string(),
            "https://dev.azure.com/test-org/project/_git/repo".to_string(),
        );
        let file_prop = ParsedProperty::File(
            "test-org".to_string(),
            PathBuf::from("path/to/file"),
            "organization = \"test-org\"".to_string(),
        );
        let default_prop = ParsedProperty::Default("test-org".to_string());

        // Test source names
        assert_eq!(cli_prop.source_name(), "cli");
        assert_eq!(env_prop.source_name(), "env");
        assert_eq!(git_prop.source_name(), "git");
        assert_eq!(file_prop.source_name(), "file");
        assert_eq!(default_prop.source_name(), "default");

        // Test original values
        assert_eq!(cli_prop.original(), Some("--organization test-org"));
        assert_eq!(env_prop.original(), Some("MERGERS_ORGANIZATION=test-org"));
        assert_eq!(
            git_prop.original(),
            Some("https://dev.azure.com/test-org/project/_git/repo")
        );
        assert_eq!(file_prop.original(), Some("organization = \"test-org\""));
        assert_eq!(default_prop.original(), None);

        // Test is_from_source
        assert!(cli_prop.is_from_source("cli"));
        assert!(!cli_prop.is_from_source("env"));
        assert!(git_prop.is_from_source("git"));
        assert!(default_prop.is_from_source("default"));
    }

    /// # ParsedProperty with Different Types
    ///
    /// Tests ParsedProperty with various data types.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty instances with String, usize, and Option types
    /// - Tests type preservation and access
    ///
    /// ## Expected Outcome
    /// - All types work correctly with ParsedProperty
    /// - Deref provides transparent access to underlying type
    #[test]
    fn test_parsed_property_different_types() {
        let string_prop = ParsedProperty::Cli("test".to_string(), "test".to_string());
        let number_prop = ParsedProperty::Env(300usize, "300".to_string());
        let option_prop = ParsedProperty::File(
            Some("test".to_string()),
            PathBuf::from("path/to/file"),
            "test".to_string(),
        );

        assert_eq!(string_prop.value(), "test");
        assert_eq!(number_prop.value(), &300usize);
        assert_eq!(option_prop.value(), &Some("test".to_string()));

        // Test deref with different operations
        assert_eq!(string_prop.len(), 4); // String method via deref
        assert_eq!(*number_prop + 100, 400); // usize operation via deref
        assert!(option_prop.is_some()); // Option method via deref
    }
}
