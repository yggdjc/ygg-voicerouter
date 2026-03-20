//! Handler trait for voice router dispatch targets.

/// A handler receives the (trigger-stripped) text payload and acts on it.
///
/// Implementations must be `Send + Sync` so that the owning [`Router`] can be
/// used from multiple threads or across `async` await points.
///
/// # Design note: synchronous interface
///
/// This trait is intentionally synchronous. The main event loop is a blocking
/// thread and blocking I/O is acceptable here. Callers running inside an async
/// runtime must not call `handle` on the runtime thread directly; use
/// [`tokio::task::spawn_blocking`] (or the equivalent for your executor) to
/// drive a handler without stalling the executor.
///
/// [`Router`]: crate::router::Router
///
/// # Examples
///
/// ```
/// use voicerouter::router::handler::Handler;
///
/// struct Echo;
///
/// impl Handler for Echo {
///     fn name(&self) -> &str { "echo" }
///     fn handle(&self, text: &str) -> anyhow::Result<()> {
///         println!("{text}");
///         Ok(())
///     }
/// }
/// ```
pub trait Handler: Send + Sync {
    /// Human-readable identifier for this handler, used in log messages.
    fn name(&self) -> &str;

    /// Process `text` — the full transcript or post-trigger payload.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler fails to process the text (e.g. network
    /// error, subprocess failure, I/O error).
    fn handle(&self, text: &str) -> anyhow::Result<()>;
}
