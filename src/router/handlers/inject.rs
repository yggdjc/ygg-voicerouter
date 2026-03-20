//! Inject handler — forwards transcribed text to the focused window.

use crate::config::InjectMethod;
use crate::inject::inject_text;
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
        inject_text(text, self.method)
    }
}
