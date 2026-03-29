//! Overlay client — sends state updates to the visual overlay process.
//!
//! Fire-and-forget: if the overlay is not running, all sends are silently
//! dropped. The client lazily connects on the first send and reconnects
//! on write failure.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

const LEVEL_THRESHOLD_LOW: f32 = 0.01;
const LEVEL_THRESHOLD_HIGH: f32 = 0.05;

#[must_use]
pub fn rms_to_level(rms: f32) -> u8 {
    if rms >= LEVEL_THRESHOLD_HIGH {
        2
    } else if rms >= LEVEL_THRESHOLD_LOW {
        1
    } else {
        0
    }
}

pub struct OverlayClient {
    stream: Option<UnixStream>,
    path: PathBuf,
    last_level: u8,
}

impl OverlayClient {
    #[must_use]
    pub fn new() -> Self {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        Self {
            stream: None,
            path: PathBuf::from(runtime_dir).join("voicerouter-overlay.sock"),
            last_level: u8::MAX,
        }
    }

    fn send_raw(&mut self, json: &str) {
        if self.stream.is_none() {
            self.stream = UnixStream::connect(&self.path).ok();
            if self.stream.is_none() {
                return;
            }
        }

        let msg = format!("{json}\n");
        if let Some(ref mut s) = self.stream {
            if s.write_all(msg.as_bytes()).is_err() {
                self.stream = None;
            }
        }
    }

    pub fn send_recording(&mut self, level: u8) {
        if level == self.last_level {
            return;
        }
        self.last_level = level;
        self.send_raw(&format!(r#"{{"state":"recording","level":{level}}}"#));
    }

    pub fn send_transcribing(&mut self) {
        self.last_level = u8::MAX;
        self.send_raw(r#"{"state":"transcribing"}"#);
    }

    pub fn send_result(&mut self, text: &str) {
        self.last_level = u8::MAX;
        let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_raw(&format!(r#"{{"state":"result","text":"{escaped}"}}"#));
    }

    pub fn send_thinking(&mut self) {
        self.last_level = u8::MAX;
        self.send_raw(r#"{"state":"thinking"}"#);
    }

    pub fn send_speaking(&mut self, text: &str) {
        self.last_level = u8::MAX;
        let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_raw(&format!(r#"{{"state":"speaking","text":"{escaped}"}}"#));
    }

    pub fn send_idle(&mut self) {
        self.last_level = u8::MAX;
        self.send_raw(r#"{"state":"idle"}"#);
    }
}

impl Default for OverlayClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_to_level_silent() {
        assert_eq!(rms_to_level(0.005), 0);
    }

    #[test]
    fn rms_to_level_soft() {
        assert_eq!(rms_to_level(0.03), 1);
    }

    #[test]
    fn rms_to_level_loud() {
        assert_eq!(rms_to_level(0.08), 2);
    }

    #[test]
    fn rms_to_level_boundary_low() {
        assert_eq!(rms_to_level(0.01), 1);
    }

    #[test]
    fn rms_to_level_boundary_high() {
        assert_eq!(rms_to_level(0.05), 2);
    }

    #[test]
    fn client_no_socket_does_not_panic() {
        let mut client = OverlayClient::new();
        client.send_recording(1);
        client.send_transcribing();
        client.send_result("hello");
        client.send_thinking();
        client.send_idle();
    }

    #[test]
    fn send_recording_deduplicates() {
        let mut client = OverlayClient::new();
        client.send_recording(1);
        assert_eq!(client.last_level, 1);
        client.send_recording(1);
        assert_eq!(client.last_level, 1);
        client.send_recording(2);
        assert_eq!(client.last_level, 2);
    }
}
