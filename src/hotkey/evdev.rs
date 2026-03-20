//! evdev device scanning and non-blocking keyboard monitoring.
//!
//! [`HotkeyMonitor`] enumerates all `/dev/input/event*` devices that report
//! key events, sets each file descriptor to non-blocking mode, and polls them
//! on every call to [`HotkeyMonitor::poll`].  A 50 ms debounce window
//! suppresses duplicate events that arise when multiple keyboards (or device
//! nodes for the same physical keyboard) fire the same key simultaneously.

use std::io::ErrorKind;
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use log::{debug, warn};

use crate::config::HotkeyConfig;
use crate::hotkey::{HotkeyEvent, HotkeyStateMachine, KeyAction};

/// Debounce window: duplicate events within this interval are suppressed.
const DEBOUNCE: Duration = Duration::from_millis(50);

/// Parse a key-name string such as `"KEY_RIGHTALT"` into an [`evdev::Key`].
///
/// Iterates over `evdev::Key`'s named variants by matching the debug
/// representation, which the evdev crate formats as the constant name.
///
/// Returns `None` if the name is not recognised.
fn parse_key(name: &str) -> Option<evdev::Key> {
    // evdev::Key implements Debug as the constant name (e.g. "KEY_RIGHTALT").
    // We iterate all codes 0..KEY_MAX and compare.
    for code in 0..768_u16 {
        let key = evdev::Key::new(code);
        if format!("{key:?}") == name {
            return Some(key);
        }
    }
    None
}

/// Open all `/dev/input/event*` devices that support the target key and set
/// them to non-blocking I/O.
fn open_keyboard_devices(target_key: evdev::Key) -> Vec<evdev::Device> {
    let mut devices = Vec::new();

    for (path, device) in evdev::enumerate() {
        let supports_key = device
            .supported_keys()
            .is_some_and(|keys| keys.contains(target_key));

        if !supports_key {
            continue;
        }

        // Set O_NONBLOCK so poll() never blocks.
        // SAFETY: AsRawFd is always valid for an open Device.
        let fd = device.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
        if flags == -1 {
            warn!("fcntl F_GETFL failed for {}", path.display());
            continue;
        }
        let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if ret == -1 {
            warn!("fcntl F_SETFL O_NONBLOCK failed for {}", path.display());
            continue;
        }

        debug!("opened keyboard device: {} ({})", path.display(), device.name().unwrap_or("?"));
        devices.push(device);
    }

    devices
}

/// Monitors keyboard devices for a configured hotkey and emits [`HotkeyEvent`]s.
///
/// Wraps [`HotkeyStateMachine`] with real evdev I/O.  Call [`Self::poll`] in
/// a tight loop or from a dedicated thread.
pub struct HotkeyMonitor {
    target_key: evdev::Key,
    devices: Vec<evdev::Device>,
    state_machine: HotkeyStateMachine,
    last_event_time: Option<Instant>,
}

impl HotkeyMonitor {
    /// Open all matching keyboard devices and initialise the state machine.
    ///
    /// # Errors
    ///
    /// Returns an error if the key name in `config` is not recognised.
    /// Individual device open failures are logged and skipped, not fatal.
    pub fn new(config: &HotkeyConfig) -> Result<Self> {
        let target_key = parse_key(&config.key)
            .with_context(|| format!("unknown key name: {}", config.key))?;

        let devices = open_keyboard_devices(target_key);
        if devices.is_empty() {
            warn!(
                "no keyboard devices found that support {}; \
                 hotkey monitoring will be inactive",
                config.key
            );
        }

        Ok(Self {
            target_key,
            devices,
            state_machine: HotkeyStateMachine::new(config.mode, config.hold_delay),
            last_event_time: None,
        })
    }

    /// Poll all devices for the next pending [`HotkeyEvent`], if any.
    ///
    /// Returns immediately (non-blocking).  Call repeatedly from a loop.
    /// Returns `None` when no actionable event is available.
    pub fn poll(&mut self) -> Option<HotkeyEvent> {
        // Drain any event buffered from a previous call (auto long-press).
        if let Some(ev) = self.state_machine.take_pending() {
            return Some(ev);
        }

        let now = Instant::now();

        for device in &mut self.devices {
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        if let evdev::InputEventKind::Key(key) = event.kind() {
                            if key != self.target_key {
                                continue;
                            }

                            let action = match event.value() {
                                1 => KeyAction::Down,
                                0 => KeyAction::Up,
                                _ => continue, // ignore repeats (value == 2)
                            };

                            // Debounce: ignore if same action within DEBOUNCE window.
                            if let Some(last) = self.last_event_time {
                                if now.duration_since(last) < DEBOUNCE {
                                    debug!("debounced {action:?} event");
                                    continue;
                                }
                            }
                            self.last_event_time = Some(now);

                            if let Some(ev) = self.state_machine.process(action, now) {
                                return Some(ev);
                            }
                        }
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    // No events available on this device right now — expected.
                }
                Err(e) => {
                    warn!("error reading evdev events: {e}");
                }
            }
        }

        None
    }
}
