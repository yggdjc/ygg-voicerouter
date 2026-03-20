//! Hotkey monitor: detects keyboard events and emits recording control signals.
//!
//! The module splits into two layers:
//!
//! - [`HotkeyStateMachine`] — a pure, I/O-free state machine that accepts raw
//!   key actions and returns [`HotkeyEvent`]s. This is the testable core.
//! - [`HotkeyMonitor`] — wraps the state machine and reads from real evdev
//!   devices found in `/dev/input/event*`.
//!
//! # Example
//!
//! ```no_run
//! use voicerouter::config::HotkeyConfig;
//! use voicerouter::hotkey::{HotkeyMonitor, HotkeyEvent};
//!
//! let config = HotkeyConfig::default();
//! let mut monitor = HotkeyMonitor::new(&config).expect("failed to open devices");
//! loop {
//!     if let Some(event) = monitor.poll() {
//!         match event {
//!             HotkeyEvent::StartRecording => println!("start"),
//!             HotkeyEvent::StopRecording  => println!("stop"),
//!         }
//!     }
//! }
//! ```

pub mod evdev;

use std::time::{Duration, Instant};

use crate::config::HotkeyMode;

// ---------------------------------------------------------------------------
// Public event type
// ---------------------------------------------------------------------------

/// High-level signal produced by the hotkey system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// The user wants to begin recording audio.
    StartRecording,
    /// The user wants to stop recording audio.
    StopRecording,
}

// ---------------------------------------------------------------------------
// Low-level key action fed into the state machine
// ---------------------------------------------------------------------------

/// A normalised key action (pressed or released).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Key pressed (value == 1 in evdev).
    Down,
    /// Key released (value == 0 in evdev).
    Up,
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// Internal state of the hotkey state machine.
#[derive(Debug, Clone)]
enum HotkeyState {
    /// No key held; not recording (toggle) or idle (PTT/auto).
    Idle,
    /// Key is currently held down. Records when it was first pressed.
    KeyDown { since: Instant },
    /// Recording is active (used in Toggle and Auto-toggle paths).
    Recording,
    /// PTT is active; waiting for key release to stop.
    WaitingRelease,
}

/// Pure state machine — no I/O, injectable timestamps for testing.
///
/// Feed `(KeyAction, Instant)` pairs via [`Self::process`]; drain any buffered
/// second event via [`Self::take_pending`] immediately after.
pub struct HotkeyStateMachine {
    mode: HotkeyMode,
    hold_delay: Duration,
    state: HotkeyState,
    /// At most one event can be buffered (auto long-press emits Start + Stop).
    pending: Option<HotkeyEvent>,
}

impl HotkeyStateMachine {
    /// Create a new state machine for the given mode and hold delay.
    ///
    /// `hold_delay_secs` is only relevant for [`HotkeyMode::Auto`].
    ///
    /// # Example
    ///
    /// ```
    /// use voicerouter::hotkey::HotkeyStateMachine;
    /// use voicerouter::config::HotkeyMode;
    ///
    /// let sm = HotkeyStateMachine::new(HotkeyMode::Ptt, 0.3);
    /// ```
    #[must_use]
    pub fn new(mode: HotkeyMode, hold_delay_secs: f64) -> Self {
        Self {
            mode,
            hold_delay: Duration::from_secs_f64(hold_delay_secs),
            state: HotkeyState::Idle,
            pending: None,
        }
    }

    /// Feed a key action at the given timestamp; returns a recording event if one fires.
    ///
    /// In Auto long-press mode a single key-up event may produce two events
    /// (`StartRecording` followed by `StopRecording`). Call [`Self::take_pending`]
    /// after each `process` call to drain the second event.
    pub fn process(&mut self, action: KeyAction, now: Instant) -> Option<HotkeyEvent> {
        match self.mode {
            HotkeyMode::Ptt => self.process_ptt(action),
            HotkeyMode::Toggle => self.process_toggle(action),
            HotkeyMode::Auto => self.process_auto(action, now),
        }
    }

    /// Drain a pending event queued during a transition that produces two events.
    ///
    /// Call this immediately after [`Self::process`] returns `Some(…)` to check
    /// whether a second event is buffered.
    pub fn take_pending(&mut self) -> Option<HotkeyEvent> {
        self.pending.take()
    }

    // -----------------------------------------------------------------------
    // Mode-specific handlers
    // -----------------------------------------------------------------------

    fn process_ptt(&mut self, action: KeyAction) -> Option<HotkeyEvent> {
        match (&self.state, action) {
            (HotkeyState::Idle, KeyAction::Down) => {
                self.state = HotkeyState::WaitingRelease;
                Some(HotkeyEvent::StartRecording)
            }
            (HotkeyState::WaitingRelease, KeyAction::Up) => {
                self.state = HotkeyState::Idle;
                Some(HotkeyEvent::StopRecording)
            }
            _ => None,
        }
    }

    fn process_toggle(&mut self, action: KeyAction) -> Option<HotkeyEvent> {
        // Toggle reacts only to key-down; ignore key-up.
        if action != KeyAction::Down {
            return None;
        }
        match &self.state {
            HotkeyState::Idle => {
                self.state = HotkeyState::Recording;
                Some(HotkeyEvent::StartRecording)
            }
            HotkeyState::Recording => {
                self.state = HotkeyState::Idle;
                Some(HotkeyEvent::StopRecording)
            }
            _ => None,
        }
    }

    fn process_auto(&mut self, action: KeyAction, now: Instant) -> Option<HotkeyEvent> {
        match (&self.state, action) {
            // Key goes down: start timing; don't emit yet.
            (HotkeyState::Idle, KeyAction::Down) => {
                self.state = HotkeyState::KeyDown { since: now };
                None
            }

            // Key released while timing — short vs long press decision.
            (HotkeyState::KeyDown { since }, KeyAction::Up) => {
                let held = now.duration_since(*since);
                if held < self.hold_delay {
                    // Short press: toggle — begin recording, wait for second press.
                    self.state = HotkeyState::Recording;
                    Some(HotkeyEvent::StartRecording)
                } else {
                    // Long press: PTT — start and immediately stop in one gesture.
                    self.state = HotkeyState::Idle;
                    self.pending = Some(HotkeyEvent::StopRecording);
                    Some(HotkeyEvent::StartRecording)
                }
            }

            // Toggle stop: second press while already recording.
            (HotkeyState::Recording, KeyAction::Down) => {
                self.state = HotkeyState::Idle;
                Some(HotkeyEvent::StopRecording)
            }

            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// HotkeyMonitor — public API wrapping state machine + evdev I/O
// ---------------------------------------------------------------------------

pub use self::evdev::HotkeyMonitor;
