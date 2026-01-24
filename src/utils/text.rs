//! Text manipulation utilities for safe UTF-8 string handling.
//!
//! This module provides helper functions for working with UTF-8 strings,
//! ensuring operations respect character boundaries to prevent panics
//! when dealing with multi-byte characters (e.g., Turkish characters like 'ı', 'ş', 'ğ').

/// Safely truncate a UTF-8 string to at most `max_bytes` bytes at a char boundary.
///
/// This function ensures the returned slice ends at a valid UTF-8 character
/// boundary, preventing panics that would occur from slicing in the middle
/// of a multi-byte character.
///
/// # Arguments
///
/// * `s` - The string slice to truncate
/// * `max_bytes` - Maximum number of bytes for the result
///
/// # Returns
///
/// A string slice that is at most `max_bytes` bytes long, ending at a valid
/// character boundary. If the string is already within the limit, returns
/// the original string unchanged.
///
/// # Example
///
/// ```
/// use mergers::utils::truncate_str;
///
/// // ASCII string - simple case
/// let text = "Hello, World!";
/// assert_eq!(truncate_str(text, 5), "Hello");
///
/// // Turkish text with multi-byte characters
/// let turkish = "Merhaba dünya";
/// let truncated = truncate_str(turkish, 10);
/// assert!(truncated.len() <= 10);
/// assert!(truncated.is_char_boundary(truncated.len()));
///
/// // String shorter than limit - returns unchanged
/// let short = "Hi";
/// assert_eq!(truncate_str(short, 10), "Hi");
/// ```
#[inline]
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the largest char boundary <= max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Test: ASCII String Truncation
    ///
    /// Verifies basic truncation works correctly for ASCII-only strings.
    ///
    /// ## Test Scenario
    /// - Creates a simple ASCII string
    /// - Truncates to various lengths
    ///
    /// ## Expected Outcome
    /// - String is truncated to exact byte count for ASCII
    #[test]
    fn test_truncate_ascii() {
        assert_eq!(truncate_str("Hello, World!", 5), "Hello");
        assert_eq!(truncate_str("Hello", 10), "Hello");
        assert_eq!(truncate_str("Hello", 5), "Hello");
        assert_eq!(truncate_str("Hello", 3), "Hel");
    }

    /// # Test: Empty String Handling
    ///
    /// Verifies empty strings are handled correctly.
    ///
    /// ## Test Scenario
    /// - Passes empty string with various max_bytes values
    ///
    /// ## Expected Outcome
    /// - Returns empty string without panicking
    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate_str("", 0), "");
        assert_eq!(truncate_str("", 10), "");
    }

    /// # Test: Multi-byte UTF-8 Character Boundary
    ///
    /// Verifies truncation respects UTF-8 character boundaries for Turkish text.
    ///
    /// ## Test Scenario
    /// - Uses Turkish text containing multi-byte characters ('ü', 'ı', 'ş', 'ğ')
    /// - Attempts to truncate at positions that would fall inside a multi-byte char
    ///
    /// ## Expected Outcome
    /// - Result is always valid UTF-8
    /// - Result length is <= max_bytes
    /// - No panic occurs
    #[test]
    fn test_truncate_turkish_characters() {
        // 'ü' is 2 bytes in UTF-8
        let text = "dünya"; // d(1) + ü(2) + n(1) + y(1) + a(1) = 6 bytes

        // Truncate at byte 2 - would be inside 'ü', should back up to 'd'
        let result = truncate_str(text, 2);
        assert_eq!(result, "d");
        assert!(result.is_char_boundary(result.len()));

        // Truncate at byte 3 - exactly after 'ü'
        let result = truncate_str(text, 3);
        assert_eq!(result, "dü");

        // Full string
        assert_eq!(truncate_str(text, 10), "dünya");
    }

    /// # Test: Zero Max Bytes
    ///
    /// Verifies zero max_bytes returns empty string.
    ///
    /// ## Test Scenario
    /// - Passes non-empty string with max_bytes = 0
    ///
    /// ## Expected Outcome
    /// - Returns empty string
    #[test]
    fn test_truncate_zero_bytes() {
        assert_eq!(truncate_str("Hello", 0), "");
        assert_eq!(truncate_str("Merhaba", 0), "");
    }

    /// # Test: Various Multi-byte Characters
    ///
    /// Verifies truncation works with various Unicode characters.
    ///
    /// ## Test Scenario
    /// - Tests with emoji (4 bytes), CJK characters (3 bytes), accented chars (2 bytes)
    ///
    /// ## Expected Outcome
    /// - All results are valid UTF-8
    /// - No panics occur
    #[test]
    fn test_truncate_various_unicode() {
        // Emoji - 4 bytes
        let emoji = "Hello 👋 World";
        let result = truncate_str(emoji, 8);
        assert!(result.len() <= 8);
        assert!(result.is_char_boundary(result.len()));

        // Mixed content
        let mixed = "café";
        assert_eq!(truncate_str(mixed, 4), "caf");
        assert_eq!(truncate_str(mixed, 5), "café");
    }

    /// # Test: Git Error Message with Turkish
    ///
    /// Verifies real-world scenario that caused the original bug.
    ///
    /// ## Test Scenario
    /// - Uses actual Turkish git error message format
    /// - Truncates at various positions
    ///
    /// ## Expected Outcome
    /// - No panic from byte boundary issues
    #[test]
    fn test_truncate_git_error_turkish() {
        let error = "hata: 8cd8822 uygulanamadı... ipucu: Çakışmaları çözdükten sonra";

        // Truncate at position that might fall inside 'ı' or 'Ç'
        for max_len in 0..error.len() {
            let result = truncate_str(error, max_len);
            assert!(result.len() <= max_len);
            // This should not panic - validates UTF-8
            let _ = result.chars().count();
        }
    }
}
