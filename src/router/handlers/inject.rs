//! Inject handler — forwards text to the focused window via the inject module.
//!
//! The inject module (Task 5) does not exist yet. This implementation stubs
//! the call with a log message. When the inject module is available, replace
//! the log call with `inject::linux::inject_text(text, self.method)`.

use crate::config::InjectMethod;
use crate::router::handler::Handler;

/// Injects transcribed text into the currently focused window.
///
/// # Examples
///
/// ```
/// use voicerouter::config::InjectMethod;
/// use voicerouter::router::handler::Handler;
/// use voicerouter::router::handlers::inject::InjectHandler;
///
/// let handler = InjectHandler::new(InjectMethod::Auto);
/// assert_eq!(handler.name(), "inject");
/// // Stub: no actual window injection happens yet.
/// handler.handle("hello world").unwrap();
/// ```
pub struct InjectHandler {
    method: InjectMethod,
}

impl InjectHandler {
    /// Create a new `InjectHandler` that will use `method` for text injection.
    #[must_use]
    pub fn new(method: InjectMethod) -> Self {
        Self { method }
    }
}

impl Handler for InjectHandler {
    fn name(&self) -> &str {
        "inject"
    }

    fn handle(&self, text: &str) -> anyhow::Result<()> {
        // STUB: inject module not yet implemented (Task 5).
        // Replace this log with inject::linux::inject_text(text, self.method).
        log::info!(
            "[inject stub] would inject via {:?}: {:?}",
            self.method,
            text
        );
        Ok(())
    }
}
