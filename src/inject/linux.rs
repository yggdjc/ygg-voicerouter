//! Linux text injection back-ends (Wayland + X11).
//!
//! Three strategies are supported, tried in priority order when `Auto` is
//! requested:
//!
//! 1. **clipboard-paste** — saves current clipboard, copies text, fires
//!    Ctrl+V via `ydotool` (Wayland) or `xdotool` (X11), then restores the
//!    original clipboard after a short delay.
//! 2. **wtype** — Wayland-only direct keystroke injection.
//! 3. **xdotool** — X11-only direct keystroke injection.

use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::config::InjectMethod;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Inject `text` using the given method.
///
/// When `method` is [`InjectMethod::Auto`], [`detect_method`] picks the best
/// available back-end.
pub fn inject(text: &str, method: InjectMethod) -> Result<()> {
    let resolved = match method {
        InjectMethod::Auto => detect_method(),
        other => other,
    };

    log::info!("inject: using method {:?}", resolved);

    match resolved {
        InjectMethod::ClipboardPaste => clipboard_paste(text),
        InjectMethod::Wtype => wtype_inject(text),
        InjectMethod::Xdotool => xdotool_inject(text),
        // Auto is resolved above; this arm is unreachable but exhaustive.
        InjectMethod::Auto => clipboard_paste(text),
    }
}

// ---------------------------------------------------------------------------
// Method detection
// ---------------------------------------------------------------------------

/// Choose the best injection method available on the current session.
///
/// Priority:
/// 1. `wtype` if running under Wayland and `wtype` is installed.
/// 2. Clipboard-paste if a clipboard tool (`wl-paste` / `xclip`) and a
///    key-injection tool (`ydotool` / `xdotool`) are present.
/// 3. `xdotool` if running under X11 and `xdotool` is installed.
/// 4. Falls back to `ClipboardPaste` as a last-resort attempt (it will fail
///    gracefully with an error at call time if nothing is installed).
pub fn detect_method() -> InjectMethod {
    let is_wayland = is_wayland_session();
    let is_x11 = is_x11_session();

    if is_wayland && is_command_available("wtype") {
        return InjectMethod::Wtype;
    }

    let has_clipboard = is_command_available("wl-paste") || is_command_available("xclip");
    let has_key_sender =
        is_command_available("ydotool") || is_command_available("xdotool");
    if has_clipboard && has_key_sender {
        return InjectMethod::ClipboardPaste;
    }

    if is_x11 && is_command_available("xdotool") {
        return InjectMethod::Xdotool;
    }

    log::warn!(
        "inject: no preferred tool found; falling back to clipboard-paste (may fail)"
    );
    InjectMethod::ClipboardPaste
}

// ---------------------------------------------------------------------------
// Back-end implementations
// ---------------------------------------------------------------------------

/// Inject text via clipboard: save → copy → paste keystroke → restore.
pub fn clipboard_paste(text: &str) -> Result<()> {
    // 1. Save current clipboard content (best-effort; empty string on failure).
    let saved = read_clipboard().unwrap_or_default();

    // 2. Copy new text to clipboard.
    write_clipboard(text).context("clipboard_paste: failed to write text to clipboard")?;

    // 3. Simulate Ctrl+V.
    send_paste_key().context("clipboard_paste: failed to send paste keystroke")?;

    // 4. Brief delay so the target app can process the paste before we clobber
    //    the clipboard again.
    thread::sleep(Duration::from_millis(100));

    // 5. Restore original clipboard content (best-effort).
    if let Err(e) = write_clipboard(&saved) {
        log::warn!("clipboard_paste: could not restore clipboard: {e}");
    }

    Ok(())
}

/// Inject text using `wtype` (Wayland).
pub fn wtype_inject(text: &str) -> Result<()> {
    run_command("wtype", &[text])
        .context("wtype_inject: wtype failed")?;
    Ok(())
}

/// Inject text using `xdotool type` (X11).
pub fn xdotool_inject(text: &str) -> Result<()> {
    run_command("xdotool", &["type", "--clearmodifiers", text])
        .context("xdotool_inject: xdotool failed")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Clipboard helpers
// ---------------------------------------------------------------------------

/// Read the current clipboard content using the best available tool.
fn read_clipboard() -> Result<String> {
    if is_command_available("wl-paste") {
        return run_command("wl-paste", &["--no-newline"]);
    }
    if is_command_available("xclip") {
        return run_command("xclip", &["-selection", "clipboard", "-o"]);
    }
    bail!("read_clipboard: neither wl-paste nor xclip is available");
}

/// Write `text` to the clipboard using the best available tool.
fn write_clipboard(text: &str) -> Result<()> {
    if is_command_available("wl-copy") {
        // wl-copy reads from stdin when no argument is given; pass via arg.
        run_command("wl-copy", &[text])?;
        return Ok(());
    }
    if is_command_available("xclip") {
        // xclip reads from stdin; use echo-free approach via process stdin.
        use std::io::Write as _;
        use std::process::{Command, Stdio};

        let mut child = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .spawn()
            .context("write_clipboard: failed to spawn xclip")?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .context("write_clipboard: failed to write to xclip stdin")?;
        }

        let status = child
            .wait()
            .context("write_clipboard: failed to wait for xclip")?;

        if !status.success() {
            bail!("write_clipboard: xclip exited with status {status}");
        }
        return Ok(());
    }
    bail!("write_clipboard: neither wl-copy nor xclip is available");
}

/// Send the Ctrl+V keystroke using the best available tool.
fn send_paste_key() -> Result<()> {
    if is_command_available("ydotool") {
        run_command("ydotool", &["key", "29:1", "47:1", "47:0", "29:0"])?;
        return Ok(());
    }
    if is_command_available("xdotool") {
        run_command("xdotool", &["key", "--clearmodifiers", "ctrl+v"])?;
        return Ok(());
    }
    bail!("send_paste_key: neither ydotool nor xdotool is available");
}

// ---------------------------------------------------------------------------
// Session detection
// ---------------------------------------------------------------------------

fn is_wayland_session() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

fn is_x11_session() -> bool {
    std::env::var("DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("x11"))
            .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Process utilities
// ---------------------------------------------------------------------------

/// Run an external command and return its stdout as a `String`.
///
/// Returns an error if the process fails to spawn, exits with non-zero status,
/// or produces non-UTF-8 output.
pub fn run_command(cmd: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("run_command: failed to spawn `{cmd}`"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "run_command: `{cmd}` exited with status {}: {stderr}",
            output.status
        );
    }

    String::from_utf8(output.stdout)
        .with_context(|| format!("run_command: `{cmd}` produced non-UTF-8 output"))
}

/// Return `true` if `cmd` can be found on `PATH`.
pub fn is_command_available(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
