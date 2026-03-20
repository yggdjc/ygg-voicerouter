//! Shell handler — executes the text payload as a shell command via `/bin/sh`.
//!
//! The command is logged before execution. Stdout and stderr are captured and
//! logged after the process exits.
//!
//! # Security note
//!
//! This handler executes arbitrary shell commands. It should only be enabled
//! for routing rules with narrow, well-understood triggers. Log lines make the
//! execution auditable but do not prevent misuse.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::router::handler::Handler;

/// Executes the payload text as a shell command.
///
/// # Examples
///
/// ```
/// use voicerouter::router::handler::Handler;
/// use voicerouter::router::handlers::shell::ShellHandler;
///
/// let handler = ShellHandler::new();
/// assert_eq!(handler.name(), "shell");
/// // Runs `echo hello` via /bin/sh.
/// handler.handle("echo hello").unwrap();
/// ```
pub struct ShellHandler;

impl ShellHandler {
    /// Create a new `ShellHandler`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShellHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for ShellHandler {
    fn name(&self) -> &str {
        "shell"
    }

    fn handle(&self, text: &str) -> Result<()> {
        let cmd = text.trim();
        if cmd.is_empty() {
            bail!("shell handler received empty command");
        }

        // Log before execution for auditability.
        log::info!("[shell] executing: {:?}", cmd);

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .context("failed to spawn shell process")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            log::info!("[shell] stdout: {}", stdout.trim_end());
        }
        if !stderr.is_empty() {
            log::warn!("[shell] stderr: {}", stderr.trim_end());
        }

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            bail!("shell command exited with status {code}: {cmd:?}");
        }

        Ok(())
    }
}
