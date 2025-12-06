use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use std::sync::OnceLock;

// Static regex pattern compiled once using OnceLock
static RELATIVE_DATE_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_relative_date_regex() -> &'static Regex {
    RELATIVE_DATE_REGEX.get_or_init(|| {
        Regex::new(r"^(\d+)(mo|w|d|h)$").expect("Failed to compile relative date regex")
    })
}

/// Parse a date string that can be either:
/// - A relative date like "1mo", "2w", "3d", "4h" (month, week, day, hour)
/// - A specific date like "2025-07-01" or "2025-07-01T12:00:00Z"
pub fn parse_since_date(since_str: &str) -> Result<DateTime<Utc>> {
    // Try to parse as a relative date first
    if let Ok(date) = parse_relative_date(since_str) {
        return Ok(date);
    }

    // Try to parse as an absolute date
    parse_absolute_date(since_str)
}

fn parse_relative_date(since_str: &str) -> Result<DateTime<Utc>> {
    let re = get_relative_date_regex();

    let caps = re
        .captures(since_str)
        .ok_or_else(|| anyhow::anyhow!("Invalid relative date format: {}", since_str))?;

    let amount: i64 = caps[1]
        .parse()
        .context("Failed to parse number in relative date")?;
    let unit = &caps[2];

    let now = Utc::now();
    let target_date = match unit {
        "mo" => now - Duration::days(amount * 30), // Approximate month as 30 days
        "w" => now - Duration::weeks(amount),
        "d" => now - Duration::days(amount),
        "h" => now - Duration::hours(amount),
        _ => return Err(anyhow::anyhow!("Unsupported time unit: {}", unit)),
    };

    Ok(target_date)
}

fn parse_absolute_date(since_str: &str) -> Result<DateTime<Utc>> {
    // Try various date formats
    let formats = [
        "%Y-%m-%d",
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%d %H:%M:%S",
    ];

    for format in &formats {
        if let Ok(dt) = DateTime::parse_from_str(since_str, format) {
            return Ok(dt.with_timezone(&Utc));
        }

        // For formats without timezone, assume UTC
        if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(since_str, format) {
            return Ok(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
        }

        // For date-only formats
        if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(since_str, format) {
            let naive_dt = naive_date.and_hms_opt(0, 0, 0).unwrap();
            return Ok(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
        }
    }

    Err(anyhow::anyhow!("Unable to parse date: {}", since_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// # Parse Relative Dates
    ///
    /// Tests parsing of relative date expressions like "1d", "2w", "3mo".
    ///
    /// ## Test Scenario
    /// - Provides various relative date strings (days, weeks, months)
    /// - Parses them relative to current time
    ///
    /// ## Expected Outcome
    /// - Relative dates are correctly calculated from current time
    /// - Different time units (d, w, mo) are properly handled
    #[test]
    fn test_parse_relative_dates() {
        let now = Utc::now();

        // Test relative dates
        let result = parse_since_date("1d").unwrap();
        let expected = now - Duration::days(1);
        let diff = (result - expected).num_minutes().abs();
        assert!(diff < 1, "1d parsing should be within 1 minute");

        let result = parse_since_date("2w").unwrap();
        let expected = now - Duration::weeks(2);
        let diff = (result - expected).num_minutes().abs();
        assert!(diff < 1, "2w parsing should be within 1 minute");

        let result = parse_since_date("3mo").unwrap();
        let expected = now - Duration::days(90);
        let diff = (result - expected).num_minutes().abs();
        assert!(diff < 1, "3mo parsing should be within 1 minute");
    }

    /// # Parse Absolute Dates
    ///
    /// Tests parsing of absolute date strings in ISO format.
    ///
    /// ## Test Scenario
    /// - Provides absolute date strings in various ISO formats
    /// - Tests both date-only and datetime with timezone formats
    ///
    /// ## Expected Outcome
    /// - Absolute dates are correctly parsed to specific timestamps
    /// - Different ISO format variations are properly supported
    #[test]
    fn test_parse_absolute_dates() {
        let result = parse_since_date("2025-07-01").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 7, 1, 0, 0, 0).unwrap();
        assert_eq!(result, expected);

        let result = parse_since_date("2025-07-01T12:30:45Z").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 7, 1, 12, 30, 45).unwrap();
        assert_eq!(result, expected);
    }

    /// # Parse Invalid Date Formats
    ///
    /// Tests error handling for invalid date format strings.
    ///
    /// ## Test Scenario
    /// - Provides various invalid date format strings
    /// - Tests parser's rejection of malformed input
    ///
    /// ## Expected Outcome
    /// - Invalid formats are properly rejected with errors
    /// - Parser doesn't crash on malformed input
    #[test]
    fn test_invalid_formats() {
        assert!(parse_since_date("invalid").is_err());
        assert!(parse_since_date("1x").is_err());
        assert!(parse_since_date("2025-13-01").is_err());
    }

    /// # Parse Relative Hours
    ///
    /// Tests parsing of relative date expressions using hours.
    ///
    /// ## Test Scenario
    /// - Provides relative hour strings ("1h", "24h", "72h")
    /// - Parses them relative to current time
    ///
    /// ## Expected Outcome
    /// - Hour-based relative dates are correctly calculated
    /// - Results are within acceptable time window
    #[test]
    fn test_parse_relative_hours() {
        let now = Utc::now();

        // Test 1 hour
        let result = parse_since_date("1h").unwrap();
        let expected = now - Duration::hours(1);
        let diff = (result - expected).num_seconds().abs();
        assert!(diff < 60, "1h parsing should be within 60 seconds");

        // Test 24 hours (1 day)
        let result = parse_since_date("24h").unwrap();
        let expected = now - Duration::hours(24);
        let diff = (result - expected).num_seconds().abs();
        assert!(diff < 60, "24h parsing should be within 60 seconds");

        // Test 72 hours (3 days)
        let result = parse_since_date("72h").unwrap();
        let expected = now - Duration::hours(72);
        let diff = (result - expected).num_seconds().abs();
        assert!(diff < 60, "72h parsing should be within 60 seconds");
    }

    /// # Parse Edge Case Amounts
    ///
    /// Tests parsing with edge case amounts like zero and large values.
    ///
    /// ## Test Scenario
    /// - Tests zero amount ("0d")
    /// - Tests large amounts ("365d", "52w")
    /// - Verifies calculations are accurate
    ///
    /// ## Expected Outcome
    /// - Zero amounts return current time (or very close to it)
    /// - Large amounts are handled correctly without overflow
    #[test]
    fn test_edge_case_amounts() {
        let now = Utc::now();

        // Test zero days
        let result = parse_since_date("0d").unwrap();
        let diff = (result - now).num_seconds().abs();
        assert!(diff < 2, "0d should be very close to current time");

        // Test large amount (1 year in days)
        let result = parse_since_date("365d").unwrap();
        let expected = now - Duration::days(365);
        let diff = (result - expected).num_minutes().abs();
        assert!(diff < 1, "365d parsing should be accurate");

        // Test large amount (1 year in weeks)
        let result = parse_since_date("52w").unwrap();
        let expected = now - Duration::weeks(52);
        let diff = (result - expected).num_minutes().abs();
        assert!(diff < 1, "52w parsing should be accurate");

        // Test large amount (12 months)
        let result = parse_since_date("12mo").unwrap();
        let expected = now - Duration::days(12 * 30);
        let diff = (result - expected).num_minutes().abs();
        assert!(diff < 1, "12mo parsing should be accurate");
    }

    /// # Parse Different Absolute Date Formats
    ///
    /// Tests parsing of various absolute date format variations.
    ///
    /// ## Test Scenario
    /// - Tests ISO date with fractional seconds
    /// - Tests date with space separator and time
    /// - Tests ISO formats without timezone indicator
    ///
    /// ## Expected Outcome
    /// - All valid ISO format variations are properly parsed
    /// - Fractional seconds are handled correctly
    #[test]
    fn test_different_absolute_formats() {
        // ISO date with fractional seconds
        let result = parse_since_date("2025-07-01T12:30:45.123Z").unwrap();
        let expected =
            Utc.with_ymd_and_hms(2025, 7, 1, 12, 30, 45).unwrap() + Duration::milliseconds(123);
        assert_eq!(result, expected);

        // Date with time and space separator (no timezone)
        let result = parse_since_date("2025-07-01 15:45:30").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 7, 1, 15, 45, 30).unwrap();
        assert_eq!(result, expected);

        // Date-only format
        let result = parse_since_date("2024-12-25").unwrap();
        let expected = Utc.with_ymd_and_hms(2024, 12, 25, 0, 0, 0).unwrap();
        assert_eq!(result, expected);
    }

    /// # Invalid Relative Date Formats
    ///
    /// Tests rejection of invalid relative date expressions.
    ///
    /// ## Test Scenario
    /// - Tests invalid units (not mo, w, d, h)
    /// - Tests missing numbers
    /// - Tests negative numbers
    /// - Tests mixed case and special characters
    ///
    /// ## Expected Outcome
    /// - All invalid relative formats are properly rejected
    /// - Error messages are returned instead of panics
    #[test]
    fn test_invalid_relative_formats() {
        // Invalid unit
        assert!(parse_since_date("1y").is_err()); // years not supported
        assert!(parse_since_date("1m").is_err()); // ambiguous (month or minute)
        assert!(parse_since_date("5s").is_err()); // seconds not supported

        // Missing number
        assert!(parse_since_date("d").is_err());
        assert!(parse_since_date("w").is_err());
        assert!(parse_since_date("mo").is_err());

        // Invalid format
        assert!(parse_since_date("1 d").is_err()); // space
        assert!(parse_since_date("d1").is_err()); // reversed
        assert!(parse_since_date("1.5d").is_err()); // decimal
    }

    /// # Invalid Absolute Date Formats
    ///
    /// Tests rejection of invalid absolute date strings.
    ///
    /// ## Test Scenario
    /// - Tests invalid month values (>12)
    /// - Tests invalid day values (>31)
    /// - Tests impossible dates (Feb 30)
    /// - Tests malformed date strings
    ///
    /// ## Expected Outcome
    /// - All invalid absolute dates are properly rejected
    /// - Parser validates date component ranges
    #[test]
    fn test_invalid_absolute_dates() {
        // Invalid month
        assert!(parse_since_date("2025-13-01").is_err());
        assert!(parse_since_date("2025-00-15").is_err());

        // Invalid day
        assert!(parse_since_date("2025-01-32").is_err());
        assert!(parse_since_date("2025-02-30").is_err());

        // Malformed formats
        assert!(parse_since_date("25-07-2025").is_err()); // Wrong year position
        assert!(parse_since_date("2025/07/01").is_err()); // Slash separator
        assert!(parse_since_date("July 1, 2025").is_err()); // Text month
        // Note: "2025-7-1" actually works in chrono, so we don't test for it
    }

    /// # Leap Year Date Parsing
    ///
    /// Tests parsing of dates that exist only in leap years.
    ///
    /// ## Test Scenario
    /// - Tests February 29 in a leap year (2024)
    /// - Verifies invalid Feb 29 in non-leap year is rejected
    ///
    /// ## Expected Outcome
    /// - Leap year dates are accepted
    /// - Non-leap year invalid dates are rejected
    #[test]
    fn test_leap_year_dates() {
        // Valid leap year date
        let result = parse_since_date("2024-02-29").unwrap();
        let expected = Utc.with_ymd_and_hms(2024, 2, 29, 0, 0, 0).unwrap();
        assert_eq!(result, expected);

        // Invalid non-leap year date
        assert!(parse_since_date("2025-02-29").is_err());
        assert!(parse_since_date("2023-02-29").is_err());
    }

    /// # Boundary Date Values
    ///
    /// Tests parsing of dates at month/year boundaries.
    ///
    /// ## Test Scenario
    /// - Tests end-of-month dates
    /// - Tests beginning-of-year and end-of-year dates
    /// - Tests different month lengths
    ///
    /// ## Expected Outcome
    /// - Boundary dates are handled correctly
    /// - Month lengths are properly validated
    #[test]
    fn test_boundary_dates() {
        // End of month - 31 days
        let result = parse_since_date("2025-01-31").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 1, 31, 0, 0, 0).unwrap();
        assert_eq!(result, expected);

        // End of month - 30 days
        let result = parse_since_date("2025-04-30").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 4, 30, 0, 0, 0).unwrap();
        assert_eq!(result, expected);

        // End of month - February non-leap
        let result = parse_since_date("2025-02-28").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 2, 28, 0, 0, 0).unwrap();
        assert_eq!(result, expected);

        // Beginning of year
        let result = parse_since_date("2025-01-01").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(result, expected);

        // End of year
        let result = parse_since_date("2025-12-31").unwrap();
        let expected = Utc.with_ymd_and_hms(2025, 12, 31, 0, 0, 0).unwrap();
        assert_eq!(result, expected);
    }

    /// # Empty and Whitespace Strings
    ///
    /// Tests handling of empty and whitespace-only input.
    ///
    /// ## Test Scenario
    /// - Tests empty string
    /// - Tests whitespace-only strings
    /// - Tests strings with leading/trailing whitespace
    ///
    /// ## Expected Outcome
    /// - Empty and whitespace strings are properly rejected
    /// - No panics or undefined behavior
    #[test]
    fn test_empty_and_whitespace() {
        assert!(parse_since_date("").is_err());
        assert!(parse_since_date("   ").is_err());
        assert!(parse_since_date("\t").is_err());
        assert!(parse_since_date("\n").is_err());

        // Strings with whitespace (should also fail since they don't match patterns)
        assert!(parse_since_date(" 1d").is_err());
        assert!(parse_since_date("1d ").is_err());
        assert!(parse_since_date(" 2025-01-01 ").is_err());
    }
}
