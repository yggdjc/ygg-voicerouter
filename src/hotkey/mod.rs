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
//!             HotkeyEvent::StartRecording  => println!("start"),
//!             HotkeyEvent::StopRecording   => println!("stop"),
//!             HotkeyEvent::CancelAndToggle => println!("cancel + toggle"),
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
    /// Auto-mode short press: cancel the tentative recording and toggle.
    /// Main loop should stop recording, discard audio, then toggle state.
    CancelAndToggle,
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
/// Feed `(KeyAction, Instant)` pairs via [`Self::process`].  In Auto mode,
/// call [`Self::tick`] periodically so that a held key transitions to PTT
/// (StartRecording) as soon as `hold_delay` elapses.
pub struct HotkeyStateMachine {
    mode: HotkeyMode,
    hold_delay: Duration,
    state: HotkeyState,
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
        }
    }

    /// Feed a key action at the given timestamp; returns a recording event if one fires.
    pub fn process(&mut self, action: KeyAction, now: Instant) -> Option<HotkeyEvent> {
        match self.mode {
            HotkeyMode::Ptt => self.process_ptt(action),
            HotkeyMode::Toggle => self.process_toggle(action),
            HotkeyMode::Auto => self.process_auto(action, now),
        }
    }

    /// Advance the state machine's time-based transitions.
    ///
    /// In Auto mode: if the key has been held for at least `hold_delay`,
    /// transition from `KeyDown` (tentative recording) to `WaitingRelease`
    /// (confirmed PTT). This commits the recording so key-up will emit
    /// `StopRecording` instead of discarding the audio.
    ///
    /// Call this on every poll iteration before processing device events.
    /// Returns `None` in all other modes or states.
    pub fn tick(&mut self, now: Instant) -> Option<HotkeyEvent> {
        if self.mode != HotkeyMode::Auto {
            return None;
        }
        if let HotkeyState::KeyDown { since } = self.state {
            if now.duration_since(since) >= self.hold_delay {
                // Commit to PTT — recording was already started on key-down.
                self.state = HotkeyState::WaitingRelease;
            }
        }
        None
    }

    /// Reset the state machine to idle. Used when an external timeout
    /// force-stops recording.
    pub fn reset(&mut self) {
        self.state = HotkeyState::Idle;
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
            // Key down from idle: start recording immediately, begin timing.
            (HotkeyState::Idle, KeyAction::Down) => {
                self.state = HotkeyState::KeyDown { since: now };
                Some(HotkeyEvent::StartRecording)
            }

            // Key released before hold_delay — short press: cancel tentative
            // recording and enter toggle mode (Recording state).
            (HotkeyState::KeyDown { .. }, KeyAction::Up) => {
                self.state = HotkeyState::Recording;
                Some(HotkeyEvent::CancelAndToggle)
            }

            // Long-press PTT release: tick() confirmed PTT; now stop normally.
            (HotkeyState::WaitingRelease, KeyAction::Up) => {
                self.state = HotkeyState::Idle;
                Some(HotkeyEvent::StopRecording)
            }

            // Toggle stop: second press while recording in toggle mode.
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
