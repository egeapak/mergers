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

    /// # ParsedProperty Display Trait
    ///
    /// Tests Display trait implementation for different ParsedProperty variants and types.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty instances with different sources and value types
    /// - Tests Display formatting through to_string()
    ///
    /// ## Expected Outcome
    /// - Display trait formats the underlying value correctly
    /// - All source variants display consistently
    #[test]
    fn test_parsed_property_display_trait() {
        let cli_string = ParsedProperty::Cli("hello".to_string(), "hello".to_string());
        let env_number = ParsedProperty::Env(42usize, "42".to_string());
        let git_string = ParsedProperty::Git("world".to_string(), "git-url".to_string());
        let file_number =
            ParsedProperty::File(123usize, PathBuf::from("config.toml"), "123".to_string());
        let default_string = ParsedProperty::Default("default".to_string());

        // Test string formatting
        assert_eq!(format!("{}", cli_string), "hello");
        assert_eq!(cli_string.to_string(), "hello");

        // Test numeric formatting
        assert_eq!(format!("{}", env_number), "42");
        assert_eq!(env_number.to_string(), "42");

        // Test different sources format consistently
        assert_eq!(format!("{}", git_string), "world");
        assert_eq!(format!("{}", file_number), "123");
        assert_eq!(format!("{}", default_string), "default");
    }

    /// # ParsedProperty Display with Special Characters
    ///
    /// Tests Display trait with edge cases and special characters.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty with empty strings, unicode, and special characters
    /// - Tests Display formatting for edge cases
    ///
    /// ## Expected Outcome
    /// - Display trait handles all string content correctly
    /// - No data loss or corruption in formatting
    #[test]
    fn test_parsed_property_display_edge_cases() {
        let empty_string = ParsedProperty::Default("".to_string());
        let unicode_string = ParsedProperty::Cli("🚀 测试".to_string(), "unicode".to_string());
        let special_chars = ParsedProperty::Env("!@#$%^&*()".to_string(), "special".to_string());
        let multiline = ParsedProperty::File(
            "line1\nline2\ntab\there".to_string(),
            PathBuf::from("file"),
            "multiline".to_string(),
        );

        assert_eq!(format!("{}", empty_string), "");
        assert_eq!(format!("{}", unicode_string), "🚀 测试");
        assert_eq!(format!("{}", special_chars), "!@#$%^&*()");
        assert_eq!(format!("{}", multiline), "line1\nline2\ntab\there");
    }

    /// # ParsedProperty AsRef<str> Trait
    ///
    /// Tests AsRef<str> trait implementation for string-based ParsedProperty instances.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty instances with String values from different sources
    /// - Tests AsRef<str> conversion and usage with str methods
    ///
    /// ## Expected Outcome
    /// - AsRef<str> provides transparent access to underlying string
    /// - All string methods work through AsRef conversion
    #[test]
    fn test_parsed_property_as_ref_str() {
        let cli_string = ParsedProperty::Cli("hello world".to_string(), "cli".to_string());
        let env_string = ParsedProperty::Env("test_value".to_string(), "env".to_string());
        let git_string = ParsedProperty::Git("repository".to_string(), "git".to_string());
        let file_string = ParsedProperty::File(
            "config_value".to_string(),
            PathBuf::from("config.toml"),
            "file".to_string(),
        );
        let default_string = ParsedProperty::Default("default_value".to_string());

        // Test AsRef<str> conversion
        assert_eq!(cli_string.as_ref(), "hello world");
        assert_eq!(env_string.as_ref(), "test_value");
        assert_eq!(git_string.as_ref(), "repository");
        assert_eq!(file_string.as_ref(), "config_value");
        assert_eq!(default_string.as_ref(), "default_value");

        // Test using string methods through AsRef
        assert!(cli_string.as_ref().contains("world"));
        assert_eq!(env_string.as_ref().len(), 10);
        assert!(git_string.as_ref().starts_with("repo"));
        assert!(file_string.as_ref().ends_with("value"));
        assert_eq!(default_string.as_ref().to_uppercase(), "DEFAULT_VALUE");
    }

    /// # ParsedProperty AsRef<str> with Functions
    ///
    /// Tests AsRef<str> trait when passing ParsedProperty to functions expecting &str.
    ///
    /// ## Test Scenario
    /// - Creates helper function that accepts AsRef<str>
    /// - Tests passing ParsedProperty instances to such functions
    ///
    /// ## Expected Outcome
    /// - ParsedProperty can be passed to functions expecting AsRef<str>
    /// - Transparent integration with existing string-based APIs
    #[test]
    fn test_parsed_property_as_ref_with_functions() {
        fn count_chars<S: AsRef<str>>(s: S) -> usize {
            s.as_ref().len()
        }

        fn contains_substring<S: AsRef<str>>(s: S, substring: &str) -> bool {
            s.as_ref().contains(substring)
        }

        let test_prop = ParsedProperty::Cli("hello world".to_string(), "test".to_string());
        let empty_prop = ParsedProperty::Default("".to_string());

        // Test with functions accepting AsRef<str>
        assert_eq!(count_chars(&test_prop), 11);
        assert_eq!(count_chars(&empty_prop), 0);
        assert!(contains_substring(&test_prop, "world"));
        assert!(!contains_substring(&test_prop, "xyz"));
        assert!(!contains_substring(&empty_prop, "anything"));
    }

    /// # ParsedProperty AsRef<str> Edge Cases
    ///
    /// Tests AsRef<str> trait with edge cases and special string content.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty with edge case string values
    /// - Tests AsRef<str> with empty strings, whitespace, and special characters
    ///
    /// ## Expected Outcome
    /// - AsRef<str> handles all string edge cases correctly
    /// - No data corruption or unexpected behavior
    #[test]
    fn test_parsed_property_as_ref_edge_cases() {
        let empty_prop = ParsedProperty::Default("".to_string());
        let whitespace_prop = ParsedProperty::Cli("   \t\n  ".to_string(), "ws".to_string());
        let unicode_prop = ParsedProperty::Env("🚀🌟".to_string(), "unicode".to_string());
        let null_char_prop = ParsedProperty::File(
            "hello\0world".to_string(),
            PathBuf::from("file"),
            "null".to_string(),
        );

        // Test empty string
        assert_eq!(empty_prop.as_ref(), "");
        assert!(empty_prop.as_ref().is_empty());

        // Test whitespace handling
        assert_eq!(whitespace_prop.as_ref(), "   \t\n  ");
        assert_eq!(whitespace_prop.as_ref().trim(), "");

        // Test unicode characters
        assert_eq!(unicode_prop.as_ref(), "🚀🌟");
        assert_eq!(unicode_prop.as_ref().chars().count(), 2);

        // Test null characters (should be preserved)
        assert_eq!(null_char_prop.as_ref(), "hello\0world");
        assert!(null_char_prop.as_ref().contains('\0'));
    }

    /// # ParsedProperty From<T> Trait
    ///
    /// Tests From<T> trait implementation for automatic conversion to Default variant.
    ///
    /// ## Test Scenario
    /// - Uses From trait to convert various types to ParsedProperty::Default
    /// - Tests both explicit and implicit conversions
    ///
    /// ## Expected Outcome
    /// - From trait creates ParsedProperty::Default variant
    /// - Automatic conversion works seamlessly
    #[test]
    fn test_parsed_property_from_trait() {
        // Test explicit From conversion
        let from_string: ParsedProperty<String> = ParsedProperty::from("hello".to_string());
        let from_usize: ParsedProperty<usize> = ParsedProperty::from(42);
        let from_bool: ParsedProperty<bool> = ParsedProperty::from(true);

        // Verify they are Default variants
        assert_eq!(from_string, ParsedProperty::Default("hello".to_string()));
        assert_eq!(from_usize, ParsedProperty::Default(42));
        assert_eq!(from_bool, ParsedProperty::Default(true));

        // Test source identification
        assert!(from_string.is_from_source("default"));
        assert!(from_usize.is_from_source("default"));
        assert!(from_bool.is_from_source("default"));

        // Test original() returns None for Default variants
        assert_eq!(from_string.original(), None);
        assert_eq!(from_usize.original(), None);
        assert_eq!(from_bool.original(), None);
    }

    /// # ParsedProperty From<T> with Into
    ///
    /// Tests From<T> trait when used with .into() conversion.
    ///
    /// ## Test Scenario
    /// - Uses .into() method to convert types to ParsedProperty
    /// - Tests type inference and implicit conversion
    ///
    /// ## Expected Outcome
    /// - Into trait (reciprocal of From) works correctly
    /// - Type inference creates appropriate ParsedProperty variants
    #[test]
    fn test_parsed_property_from_with_into() {
        // Test .into() conversion (which uses From trait)
        let string_prop: ParsedProperty<String> = "test".to_string().into();
        let number_prop: ParsedProperty<i32> = 100.into();
        let option_prop: ParsedProperty<Option<String>> = Some("value".to_string()).into();

        // Verify conversions created Default variants
        assert_eq!(string_prop, ParsedProperty::Default("test".to_string()));
        assert_eq!(number_prop, ParsedProperty::Default(100));
        assert_eq!(
            option_prop,
            ParsedProperty::Default(Some("value".to_string()))
        );

        // Test value access
        assert_eq!(string_prop.value(), "test");
        assert_eq!(*number_prop.value(), 100);
        assert_eq!(option_prop.value(), &Some("value".to_string()));
    }

    /// # ParsedProperty From<T> with Complex Types
    ///
    /// Tests From<T> trait with more complex data types.
    ///
    /// ## Test Scenario
    /// - Uses From trait with Vec, HashMap, and custom types
    /// - Tests that complex types are properly wrapped
    ///
    /// ## Expected Outcome
    /// - From trait handles complex types correctly
    /// - All operations work on wrapped complex types
    #[test]
    fn test_parsed_property_from_complex_types() {
        use std::collections::HashMap;

        // Test with Vec
        let vec_prop: ParsedProperty<Vec<String>> = vec!["a".to_string(), "b".to_string()].into();
        assert_eq!(vec_prop.len(), 2);
        assert!(vec_prop.contains(&"a".to_string()));

        // Test with HashMap
        let mut map = HashMap::new();
        map.insert("key1".to_string(), "value1".to_string());
        map.insert("key2".to_string(), "value2".to_string());
        let map_prop: ParsedProperty<HashMap<String, String>> = map.into();
        assert_eq!(map_prop.len(), 2);
        assert_eq!(map_prop.get("key1"), Some(&"value1".to_string()));

        // Test with tuple
        let tuple_prop: ParsedProperty<(String, usize)> = ("test".to_string(), 42).into();
        assert_eq!(tuple_prop.0, "test");
        assert_eq!(tuple_prop.1, 42);

        // All should be Default variants
        assert!(vec_prop.is_from_source("default"));
        assert!(map_prop.is_from_source("default"));
        assert!(tuple_prop.is_from_source("default"));
    }

    /// # ParsedProperty Deref Operations
    ///
    /// Tests Deref trait implementation for transparent access to underlying values.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty instances with different types
    /// - Tests method calls and operations through Deref
    ///
    /// ## Expected Outcome
    /// - Deref provides transparent access to underlying value
    /// - All type methods work through dereferencing
    #[test]
    fn test_parsed_property_deref_operations() {
        let string_prop = ParsedProperty::Cli("hello world".to_string(), "cli".to_string());
        let vec_prop = ParsedProperty::Env(vec![1, 2, 3, 4, 5], "env".to_string());
        let number_prop = ParsedProperty::File(42usize, PathBuf::from("file"), "42".to_string());

        // Test string methods through deref
        assert_eq!(string_prop.len(), 11);
        assert!(string_prop.contains("world"));
        assert!(string_prop.starts_with("hello"));
        assert!(string_prop.ends_with("world"));
        assert_eq!(string_prop.to_uppercase(), "HELLO WORLD");

        // Test Vec methods through deref
        assert_eq!(vec_prop.len(), 5);
        assert!(vec_prop.contains(&3));
        assert_eq!(vec_prop[0], 1);
        assert_eq!(vec_prop.iter().sum::<i32>(), 15);

        // Test numeric operations through deref
        assert_eq!(*number_prop, 42);
        assert_eq!(*number_prop + 8, 50);
        assert_eq!(*number_prop * 2, 84);
        assert!(*number_prop > 40);
    }

    /// # ParsedProperty Deref with Collections
    ///
    /// Tests Deref trait with collection types and their methods.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty with HashMap and Vec
    /// - Tests collection operations through Deref
    ///
    /// ## Expected Outcome
    /// - Collection methods work transparently through Deref
    /// - Mutability restrictions are respected
    #[test]
    fn test_parsed_property_deref_collections() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        map.insert("key1".to_string(), "value1".to_string());
        map.insert("key2".to_string(), "value2".to_string());
        map.insert("key3".to_string(), "value3".to_string());

        let map_prop = ParsedProperty::Git(map, "git".to_string());
        let vec_prop = ParsedProperty::Default(vec!["a", "b", "c", "d"]);

        // Test HashMap methods through deref
        assert_eq!(map_prop.len(), 3);
        assert!(map_prop.contains_key("key1"));
        assert_eq!(map_prop.get("key2"), Some(&"value2".to_string()));
        assert_eq!(map_prop.keys().count(), 3);
        assert_eq!(map_prop.values().count(), 3);

        // Test Vec methods through deref
        assert_eq!(vec_prop.len(), 4);
        assert!(vec_prop.contains(&"c"));
        assert_eq!(vec_prop.first(), Some(&"a"));
        assert_eq!(vec_prop.last(), Some(&"d"));

        // Test iteration through deref
        let mut collected: Vec<&str> = vec_prop.iter().copied().collect();
        collected.sort();
        assert_eq!(collected, vec!["a", "b", "c", "d"]);
    }

    /// # ParsedProperty Deref with Option Types
    ///
    /// Tests Deref trait with Option types and their methods.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty with Some and None Option values
    /// - Tests Option methods through Deref
    ///
    /// ## Expected Outcome
    /// - Option methods work correctly through Deref
    /// - Pattern matching and unwrapping work as expected
    #[test]
    fn test_parsed_property_deref_option_types() {
        let some_prop = ParsedProperty::Cli(Some("test_value".to_string()), "some".to_string());
        let none_prop = ParsedProperty::Env(None::<String>, "none".to_string());

        // Test Option methods on Some through deref
        assert!(some_prop.is_some());
        assert!(!some_prop.is_none());
        assert_eq!(some_prop.as_ref(), Some(&"test_value".to_string()));

        // Test Option methods on None through deref
        assert!(!none_prop.is_some());
        assert!(none_prop.is_none());
        assert_eq!(none_prop.as_ref(), None);

        // Test pattern matching
        match &*some_prop {
            Some(value) => assert_eq!(value, "test_value"),
            None => panic!("Expected Some value"),
        }

        match &*none_prop {
            Some(_) => panic!("Expected None value"),
            None => { /* expected */ }
        }

        // Test unwrap_or through deref (need to use as_ref() to avoid moving)
        assert_eq!(
            some_prop.as_ref().unwrap_or(&"default".to_string()),
            &"test_value".to_string()
        );
        assert_eq!(
            none_prop.as_ref().unwrap_or(&"default".to_string()),
            &"default".to_string()
        );
    }

    /// # ParsedProperty Deref Immutability
    ///
    /// Tests that Deref provides read-only access to the underlying value.
    ///
    /// ## Test Scenario
    /// - Creates ParsedProperty instances with various types
    /// - Tests that deref operations don't allow mutation
    ///
    /// ## Expected Outcome
    /// - Deref provides immutable reference to underlying value
    /// - Value cannot be modified through deref operations
    #[test]
    fn test_parsed_property_deref_immutability() {
        let string_prop = ParsedProperty::Default("test".to_string());
        let vec_prop = ParsedProperty::Cli(vec![1, 2, 3], "cli".to_string());

        // These operations should work (read-only)
        let _len = string_prop.len();
        let _chars: Vec<char> = string_prop.chars().collect();
        let _first = vec_prop.first();
        let _iter_sum: i32 = vec_prop.iter().sum();

        // Note: Mutation operations would be compile-time errors
        // The following would not compile (demonstrating immutability):
        // string_prop.push('x'); // Error: cannot borrow as mutable
        // vec_prop.push(4);      // Error: cannot borrow as mutable

        // Verify values are unchanged
        assert_eq!(&*string_prop, "test");
        assert_eq!(&*vec_prop, &vec![1, 2, 3]);
    }
}
