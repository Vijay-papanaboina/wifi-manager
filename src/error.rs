//! Application-facing errors.
//!
//! Backend adapters may still expose their native error types at their narrow
//! boundary, but user actions and process integrations use this type so error
//! context is retained instead of being flattened into an unstructured string.

use std::fmt;
pub(crate) type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub(crate) enum AppError {
    Io { operation: &'static str, source: std::io::Error },
    CommandFailed { program: String, status: i32, stderr: String },
    Process { operation: &'static str, source: String },
    Message(String),
}

impl AppError {
    pub(crate) fn command_failed(program: &str, status: i32, stderr: impl Into<String>) -> Self {
        Self::CommandFailed { program: program.to_string(), status, stderr: stderr.into() }
    }

    pub(crate) fn process(operation: &'static str, source: impl Into<String>) -> Self {
        Self::Process { operation, source: source.into() }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::CommandFailed { program, status, stderr } => {
                if stderr.is_empty() {
                    write!(f, "{program} exited with status {status}")
                } else {
                    write!(f, "{program} exited with status {status}: {stderr}")
                }
            }
            Self::Process { operation, source } => write!(f, "{operation}: {source}"),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::CommandFailed { .. } | Self::Process { .. } | Self::Message(_) => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(source: std::io::Error) -> Self {
        Self::Io { operation: "I/O operation", source }
    }
}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for AppError {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_operation_context_for_io_errors() {
        let error = AppError::Io {
            operation: "read fixture",
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        assert_eq!(error.to_string(), "read fixture: missing");
        assert!(std::error::Error::source(&error).is_some());
    }
}
