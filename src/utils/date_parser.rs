use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;

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
    let re = Regex::new(r"^(\d+)(mo|w|d|h)$")
        .context("Failed to create regex for relative date parsing")?;

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
}
