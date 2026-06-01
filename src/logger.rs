//! Process-wide logger initialisation built on [`tracing`].
//!
//! The public surface retains the original `LogLevel` type so that the CLI
//! flag / env-var parsing is unchanged.  Call [`init`] once at startup to
//! configure the global tracing subscriber.

use std::fmt;

use tracing::Level;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
}

impl LogLevel {
    pub const VARIANTS: [LogLevel; 4] = [
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warning,
        LogLevel::Error,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARNING",
            LogLevel::Error => "ERROR",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid log level \"{value}\". Must be one of: DEBUG, INFO, WARNING, ERROR.")]
pub struct ParseLogLevelError {
    pub value: String,
}

pub fn parse_log_level(value: &str) -> Result<LogLevel, ParseLogLevelError> {
    match value.to_ascii_uppercase().as_str() {
        "DEBUG" => Ok(LogLevel::Debug),
        "INFO" => Ok(LogLevel::Info),
        "WARNING" => Ok(LogLevel::Warning),
        "ERROR" => Ok(LogLevel::Error),
        _ => Err(ParseLogLevelError {
            value: value.to_owned(),
        }),
    }
}

/// Initialise the global tracing subscriber with the given minimum level.
///
/// Must be called once before any `tracing` macros are used.
pub fn init(level: LogLevel) {
    let tracing_level = match level {
        LogLevel::Debug => Level::DEBUG,
        LogLevel::Info => Level::INFO,
        LogLevel::Warning => Level::WARN,
        LogLevel::Error => Level::ERROR,
    };
    let filter = EnvFilter::builder()
        .with_default_directive(tracing_level.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_level_accepts_known_levels() {
        assert_eq!(parse_log_level("DEBUG").unwrap(), LogLevel::Debug);
        assert_eq!(parse_log_level("info").unwrap(), LogLevel::Info);
        assert_eq!(parse_log_level("Warning").unwrap(), LogLevel::Warning);
        assert_eq!(parse_log_level("ERROR").unwrap(), LogLevel::Error);
    }

    #[test]
    fn parse_log_level_rejects_unknown() {
        assert!(parse_log_level("TRACE").is_err());
    }

    #[test]
    fn ordering_matches_severity() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Error);
    }
}
