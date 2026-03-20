//! Text injection — routes transcribed text into the focused window.
//!
//! On Linux, three back-ends are available:
//!
//! | Method            | Tool(s) required              | Session |
//! |-------------------|-------------------------------|---------|
//! | `ClipboardPaste`  | `wl-copy`/`xclip` + `ydotool`/`xdotool` | any |
//! | `Wtype`           | `wtype`                       | Wayland |
//! | `Xdotool`         | `xdotool`                     | X11     |
//!
//! `Auto` (the default) picks the best available back-end at runtime.
//!
//! # Example
//!
//! ```no_run
//! use voicerouter::config::InjectMethod;
//! use voicerouter::inject::inject_text;
//!
//! inject_text("hello world", InjectMethod::Auto).expect("injection failed");
//! ```

#[cfg(target_os = "linux")]
pub mod linux;

use anyhow::Result;

use crate::config::InjectMethod;

/// Inject `text` into the currently focused window using `method`.
///
/// When `method` is [`InjectMethod::Auto`] the best available back-end is
/// selected automatically by inspecting the running display session and
/// installed tools.
///
/// # Errors
///
/// Returns an error if the selected tool is missing, fails to launch, or exits
/// with a non-zero status.
pub fn inject_text(text: &str, method: InjectMethod) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::inject(text, method)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (text, method);
        anyhow::bail!("inject_text: text injection is only supported on Linux");
    }
}
