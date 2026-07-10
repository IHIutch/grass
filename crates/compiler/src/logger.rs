use codemap::SpanLoc;
use std::fmt::Debug;

/// A trait to allow replacing logging mechanisms
pub trait Logger: Debug {
    /// Logs message from a [`@debug`](https://sass-lang.com/documentation/at-rules/debug/)
    /// statement
    fn debug(&self, location: SpanLoc, message: &str);

    /// Logs message from a [`@warn`](https://sass-lang.com/documentation/at-rules/warn/)
    /// statement
    fn warn(&self, location: SpanLoc, message: &str);

    /// Logs a deprecation warning, identified by `deprecation_id` (e.g.
    /// `"slash-div"`).
    ///
    /// The default implementation formats dart-sass's
    /// `DEPRECATION WARNING [id]: ` prefix onto `message` and delegates to
    /// [`Logger::warn`], so existing implementations of this trait keep
    /// working unchanged. Override this if you need to distinguish
    /// deprecation warnings from other warnings (e.g. to route them
    /// differently, or to format them without [`Logger::warn`]'s framing).
    fn warn_deprecation(&self, location: SpanLoc, message: &str, deprecation_id: &str) {
        self.warn(
            location,
            &format!("DEPRECATION WARNING [{deprecation_id}]: {message}"),
        );
    }
}

/// Logs events to standard error, through [`eprintln!`]
#[derive(Debug)]
pub struct StdLogger;

impl Logger for StdLogger {
    #[inline]
    fn debug(&self, location: SpanLoc, message: &str) {
        eprintln!(
            "{}:{} DEBUG: {}",
            location.file.name(),
            location.begin.line + 1,
            message
        );
    }

    #[inline]
    fn warn(&self, location: SpanLoc, message: &str) {
        eprintln!(
            "Warning: {}\n    ./{}:{}:{}",
            message,
            location.file.name(),
            location.begin.line + 1,
            location.begin.column + 1
        );
    }

    #[inline]
    fn warn_deprecation(&self, location: SpanLoc, message: &str, deprecation_id: &str) {
        eprintln!(
            "DEPRECATION WARNING [{}]: {}\n    ./{}:{}:{}",
            deprecation_id,
            message,
            location.file.name(),
            location.begin.line + 1,
            location.begin.column + 1
        );
    }
}

/// Discards all logs
#[derive(Debug)]
pub struct NullLogger;

impl Logger for NullLogger {
    #[inline]
    fn debug(&self, _location: SpanLoc, _message: &str) {}

    #[inline]
    fn warn(&self, _location: SpanLoc, _message: &str) {}
}
