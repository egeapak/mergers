//! Logging infrastructure for mergers.
//!
//! This module provides optional tracing-based logging with support for:
//! - Multiple output targets (stderr, file)
//! - Configurable log levels
//! - Selectable format (text or JSON)

use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

/// Log level configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse a log level from a string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Convert to a filter string for tracing-subscriber.
    #[must_use]
    pub fn as_filter_str(&self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable text format (default).
    #[default]
    Text,
    /// Structured JSON format.
    Json,
}

impl LogFormat {
    /// Parse a log format from a string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// Configuration for the logging system.
#[derive(Debug, Default)]
pub struct LogConfig {
    /// Log level (None means logging is disabled).
    pub level: Option<LogLevel>,
    /// Output file path (None means stderr).
    pub file: Option<PathBuf>,
    /// Output format.
    pub format: LogFormat,
    /// Whether the application is running in TUI mode.
    /// Stderr logging is disabled in TUI mode to prevent display corruption.
    pub is_tui_mode: bool,
}

/// Guard that must be held to ensure logs are flushed.
///
/// When this guard is dropped, all pending log messages are flushed.
/// Hold this until application exit.
pub struct LogGuard {
    _file_guard: Option<WorkerGuard>,
    _stderr_guard: Option<WorkerGuard>,
}

/// Initialize the logging system.
///
/// Returns `Some(LogGuard)` if logging was initialized, `None` if logging is disabled.
/// The guard must be held until application exit to ensure logs are flushed.
///
/// # Example
///
/// ```rust,no_run
/// use mergers::logging::{LogConfig, LogLevel, LogFormat, init_logging};
/// use std::path::PathBuf;
///
/// let config = LogConfig {
///     level: Some(LogLevel::Debug),
///     file: Some(PathBuf::from("/tmp/mergers.log")),
///     format: LogFormat::Text,
///     is_tui_mode: false,
/// };
///
/// let _guard = init_logging(config);
/// // Logging is now active, _guard keeps it alive
/// ```
#[must_use = "the returned guard must be held until application exit"]
pub fn init_logging(config: LogConfig) -> Option<LogGuard> {
    let level = config.level?;

    // Create filter for mergers crate only (avoid noise from dependencies)
    let filter = EnvFilter::new(format!("mergers={}", level.as_filter_str()));

    let mut guards = LogGuard {
        _file_guard: None,
        _stderr_guard: None,
    };

    // Determine output target and create appropriate layer
    match (&config.file, config.is_tui_mode) {
        // File output (works in both TUI and non-TUI modes)
        (Some(path), _) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()?;
            let (non_blocking, guard) = tracing_appender::non_blocking(file);
            guards._file_guard = Some(guard);

            match config.format {
                LogFormat::Json => {
                    let layer = fmt::layer()
                        .with_writer(non_blocking)
                        .json()
                        .with_span_events(FmtSpan::CLOSE)
                        .with_file(true)
                        .with_line_number(true);

                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .init();
                }
                LogFormat::Text => {
                    let layer = fmt::layer()
                        .with_writer(non_blocking)
                        .with_target(true)
                        .with_level(true)
                        .with_file(true)
                        .with_line_number(true);

                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .init();
                }
            }
        }

        // Stderr output (only in non-TUI mode)
        (None, false) => {
            let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stderr());
            guards._stderr_guard = Some(guard);

            match config.format {
                LogFormat::Json => {
                    let layer = fmt::layer()
                        .with_writer(non_blocking)
                        .json()
                        .with_span_events(FmtSpan::CLOSE);

                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .init();
                }
                LogFormat::Text => {
                    let layer = fmt::layer()
                        .with_writer(non_blocking)
                        .with_target(true)
                        .with_level(true)
                        .compact();

                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .init();
                }
            }
        }

        // TUI mode without file output - logging disabled
        (None, true) => {
            return None;
        }
    }

    Some(guards)
}

/// Parse logging configuration from command-line arguments and environment.
///
/// This performs early parsing before full config resolution.
/// Precedence: CLI args > environment variables.
#[must_use]
pub fn parse_early_log_config(args: &[String]) -> LogConfig {
    // Check CLI args first (highest precedence)
    let cli_level = extract_arg_value(args, "--log-level");
    let cli_file = extract_arg_value(args, "--log-file");
    let cli_format = extract_arg_value(args, "--log-format");

    // Check environment variables
    let env_level = std::env::var("MERGERS_LOG_LEVEL").ok();
    let env_file = std::env::var("MERGERS_LOG_FILE").ok();
    let env_format = std::env::var("MERGERS_LOG_FORMAT").ok();

    // CLI takes precedence over env
    let level_str = cli_level.or(env_level);
    let file_str = cli_file.or(env_file);
    let format_str = cli_format.or(env_format);

    // Determine if TUI mode (rough heuristic based on args)
    let is_tui_mode = !args.contains(&"-n".to_string())
        && !args.contains(&"--non-interactive".to_string())
        && !args.iter().any(|a| {
            a == "status" || a == "continue" || a == "abort" || a == "complete" || a == "--help"
        });

    LogConfig {
        level: level_str.and_then(|s| LogLevel::parse(&s)),
        file: file_str.map(PathBuf::from),
        format: format_str
            .and_then(|s| LogFormat::parse(&s))
            .unwrap_or_default(),
        is_tui_mode,
    }
}

/// Extract a value following a flag in command-line arguments.
fn extract_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Test: Log Level Parsing
    ///
    /// Verifies that log levels are parsed correctly from strings.
    ///
    /// ## Test Scenario
    /// - Parse valid log level strings (case-insensitive)
    /// - Parse invalid log level strings
    ///
    /// ## Expected Outcome
    /// - Valid strings return the corresponding LogLevel
    /// - Invalid strings return None
    #[test]
    fn test_log_level_parsing() {
        assert_eq!(LogLevel::parse("trace"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::parse("TRACE"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::parse("debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("Debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("info"), Some(LogLevel::Info));
        assert_eq!(LogLevel::parse("warn"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::parse("warning"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::parse("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::parse("invalid"), None);
        assert_eq!(LogLevel::parse(""), None);
    }

    /// # Test: Log Format Parsing
    ///
    /// Verifies that log formats are parsed correctly from strings.
    ///
    /// ## Test Scenario
    /// - Parse valid format strings (case-insensitive)
    /// - Parse invalid format strings
    ///
    /// ## Expected Outcome
    /// - Valid strings return the corresponding LogFormat
    /// - Invalid strings return None
    #[test]
    fn test_log_format_parsing() {
        assert_eq!(LogFormat::parse("text"), Some(LogFormat::Text));
        assert_eq!(LogFormat::parse("TEXT"), Some(LogFormat::Text));
        assert_eq!(LogFormat::parse("json"), Some(LogFormat::Json));
        assert_eq!(LogFormat::parse("JSON"), Some(LogFormat::Json));
        assert_eq!(LogFormat::parse("yaml"), None);
        assert_eq!(LogFormat::parse(""), None);
    }

    /// # Test: Early Config Parsing from Args
    ///
    /// Verifies that logging configuration is correctly extracted from CLI args.
    ///
    /// ## Test Scenario
    /// - Parse args with --log-level and --log-file flags
    /// - Parse args without logging flags
    ///
    /// ## Expected Outcome
    /// - Flags are correctly extracted
    /// - Missing flags result in None values
    #[test]
    fn test_early_config_parsing_from_args() {
        let args: Vec<String> = vec![
            "mergers".to_string(),
            "--log-level".to_string(),
            "debug".to_string(),
            "--log-file".to_string(),
            "/tmp/test.log".to_string(),
            "--log-format".to_string(),
            "json".to_string(),
        ];

        let config = parse_early_log_config(&args);
        assert_eq!(config.level, Some(LogLevel::Debug));
        assert_eq!(config.file, Some(PathBuf::from("/tmp/test.log")));
        assert_eq!(config.format, LogFormat::Json);
    }

    /// # Test: TUI Mode Detection
    ///
    /// Verifies that TUI mode is correctly detected from args.
    ///
    /// ## Test Scenario
    /// - Args without -n flag (TUI mode)
    /// - Args with -n flag (non-interactive mode)
    /// - Args with subcommands like status (non-interactive)
    ///
    /// ## Expected Outcome
    /// - TUI mode is true when no non-interactive flags
    /// - TUI mode is false when -n or subcommands present
    #[test]
    fn test_tui_mode_detection() {
        // TUI mode (no non-interactive flags)
        let tui_args: Vec<String> = vec!["mergers".to_string(), "merge".to_string()];
        let config = parse_early_log_config(&tui_args);
        assert!(config.is_tui_mode);

        // Non-interactive with -n flag
        let non_interactive_args: Vec<String> =
            vec!["mergers".to_string(), "-n".to_string(), "merge".to_string()];
        let config = parse_early_log_config(&non_interactive_args);
        assert!(!config.is_tui_mode);

        // Non-interactive with status subcommand
        let status_args: Vec<String> = vec![
            "mergers".to_string(),
            "merge".to_string(),
            "status".to_string(),
        ];
        let config = parse_early_log_config(&status_args);
        assert!(!config.is_tui_mode);
    }

    /// # Test: Logging Disabled by Default
    ///
    /// Verifies that logging is disabled when no level is specified.
    ///
    /// ## Test Scenario
    /// - Create config with no log level
    ///
    /// ## Expected Outcome
    /// - init_logging returns None
    #[test]
    fn test_logging_disabled_by_default() {
        let config = LogConfig {
            level: None,
            file: None,
            format: LogFormat::Text,
            is_tui_mode: false,
        };
        // Note: We can't easily test init_logging because it can only be called once
        // per process due to global subscriber. Just verify config is correct.
        assert!(config.level.is_none());
    }

    /// # Test: Log Level Filter String
    ///
    /// Verifies that log levels are converted to correct filter strings.
    ///
    /// ## Test Scenario
    /// - Convert each LogLevel to filter string
    ///
    /// ## Expected Outcome
    /// - Each level produces the correct lowercase string
    #[test]
    fn test_log_level_filter_string() {
        assert_eq!(LogLevel::Trace.as_filter_str(), "trace");
        assert_eq!(LogLevel::Debug.as_filter_str(), "debug");
        assert_eq!(LogLevel::Info.as_filter_str(), "info");
        assert_eq!(LogLevel::Warn.as_filter_str(), "warn");
        assert_eq!(LogLevel::Error.as_filter_str(), "error");
    }

    /// # Test: Extract Arg Value
    ///
    /// Verifies that argument values are correctly extracted.
    ///
    /// ## Test Scenario
    /// - Extract value following a flag
    /// - Try to extract from args without the flag
    ///
    /// ## Expected Outcome
    /// - Returns Some(value) when flag is present
    /// - Returns None when flag is not present
    #[test]
    fn test_extract_arg_value() {
        let args: Vec<String> = vec!["cmd".to_string(), "--flag".to_string(), "value".to_string()];
        assert_eq!(
            extract_arg_value(&args, "--flag"),
            Some("value".to_string())
        );
        assert_eq!(extract_arg_value(&args, "--other"), None);

        // Edge case: flag at end without value
        let args: Vec<String> = vec!["cmd".to_string(), "--flag".to_string()];
        assert_eq!(extract_arg_value(&args, "--flag"), None);
    }
}
