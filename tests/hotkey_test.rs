//! Integration tests for the hotkey state machine.
//!
//! All tests operate on [`HotkeyStateMachine`] directly — no hardware required.
//! Timestamps are constructed via `Instant::now()` plus synthetic offsets so the
//! tests are deterministic and fast.

use std::time::{Duration, Instant};

use voicerouter::config::HotkeyMode;
use voicerouter::hotkey::{HotkeyEvent, HotkeyStateMachine, KeyAction};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ptt() -> HotkeyStateMachine {
    HotkeyStateMachine::new(HotkeyMode::Ptt, 0.3)
}

fn toggle() -> HotkeyStateMachine {
    HotkeyStateMachine::new(HotkeyMode::Toggle, 0.3)
}

fn auto_sm() -> HotkeyStateMachine {
    HotkeyStateMachine::new(HotkeyMode::Auto, 0.3)
}

/// Return a fixed base `Instant` (now) plus `delta_ms` milliseconds.
fn at(base: Instant, delta_ms: u64) -> Instant {
    base + Duration::from_millis(delta_ms)
}

// ---------------------------------------------------------------------------
// PTT mode
// ---------------------------------------------------------------------------

#[test]
fn ptt_key_down_starts_recording() {
    let mut sm = ptt();
    let now = Instant::now();
    let result = sm.process(KeyAction::Down, now);
    assert_eq!(result, Some(HotkeyEvent::StartRecording));
}

#[test]
fn ptt_key_up_stops_recording() {
    let mut sm = ptt();
    let now = Instant::now();
    sm.process(KeyAction::Down, now);
    let result = sm.process(KeyAction::Up, at(now, 200));
    assert_eq!(result, Some(HotkeyEvent::StopRecording));
}

#[test]
fn ptt_key_up_without_down_is_ignored() {
    let mut sm = ptt();
    let now = Instant::now();
    let result = sm.process(KeyAction::Up, now);
    assert_eq!(result, None);
}

#[test]
fn ptt_double_down_ignored() {
    let mut sm = ptt();
    let now = Instant::now();
    sm.process(KeyAction::Down, now);
    // Second down while WaitingRelease — should be ignored.
    let result = sm.process(KeyAction::Down, at(now, 10));
    assert_eq!(result, None);
}

// ---------------------------------------------------------------------------
// Toggle mode
// ---------------------------------------------------------------------------

#[test]
fn toggle_first_press_starts() {
    let mut sm = toggle();
    let now = Instant::now();
    let result = sm.process(KeyAction::Down, now);
    assert_eq!(result, Some(HotkeyEvent::StartRecording));
}

#[test]
fn toggle_second_press_stops() {
    let mut sm = toggle();
    let now = Instant::now();
    sm.process(KeyAction::Down, now);
    // Key-up between presses should be ignored.
    sm.process(KeyAction::Up, at(now, 50));
    let result = sm.process(KeyAction::Down, at(now, 500));
    assert_eq!(result, Some(HotkeyEvent::StopRecording));
}

#[test]
fn toggle_key_up_ignored() {
    let mut sm = toggle();
    let now = Instant::now();
    // Key-up before any down — no event.
    let result = sm.process(KeyAction::Up, now);
    assert_eq!(result, None);
}

#[test]
fn toggle_three_presses_cycle() {
    let mut sm = toggle();
    let now = Instant::now();
    assert_eq!(sm.process(KeyAction::Down, at(now, 0)), Some(HotkeyEvent::StartRecording));
    sm.process(KeyAction::Up, at(now, 50));
    assert_eq!(sm.process(KeyAction::Down, at(now, 500)), Some(HotkeyEvent::StopRecording));
    sm.process(KeyAction::Up, at(now, 550));
    assert_eq!(sm.process(KeyAction::Down, at(now, 1000)), Some(HotkeyEvent::StartRecording));
}

// ---------------------------------------------------------------------------
// Auto mode — short press (toggle)
// ---------------------------------------------------------------------------

#[test]
fn auto_short_press_starts_then_cancels_to_toggle() {
    let mut sm = auto_sm();
    let now = Instant::now();
    // Key down: StartRecording fires immediately.
    assert_eq!(sm.process(KeyAction::Down, at(now, 0)), Some(HotkeyEvent::StartRecording));
    // Key up before hold_delay (100 ms < 300 ms): CancelAndToggle.
    // Main loop should discard tentative audio and restart in toggle mode.
    assert_eq!(sm.process(KeyAction::Up, at(now, 100)), Some(HotkeyEvent::CancelAndToggle));
    // No tick-triggered event for a short press.
    assert_eq!(sm.tick(at(now, 400)), None);
}

#[test]
fn auto_short_press_second_stops() {
    let mut sm = auto_sm();
    let now = Instant::now();
    // First short press: start then cancel-to-toggle.
    sm.process(KeyAction::Down, at(now, 0));
    sm.process(KeyAction::Up, at(now, 100));
    // Second press: stop recording (toggle off).
    let result = sm.process(KeyAction::Down, at(now, 600));
    assert_eq!(result, Some(HotkeyEvent::StopRecording));
}

// ---------------------------------------------------------------------------
// Auto mode — long press (PTT)
// ---------------------------------------------------------------------------

#[test]
fn auto_long_press_is_ptt() {
    let mut sm = auto_sm();
    let now = Instant::now();
    // Key down at t=0: StartRecording fires immediately.
    assert_eq!(sm.process(KeyAction::Down, at(now, 0)), Some(HotkeyEvent::StartRecording));
    // Tick at 400 ms: hold_delay elapsed → commits to PTT (no new event).
    assert_eq!(sm.tick(at(now, 400)), None);
    // Key-up: StopRecording fires.
    assert_eq!(sm.process(KeyAction::Up, at(now, 400)), Some(HotkeyEvent::StopRecording));
}

#[test]
fn auto_long_press_exactly_at_boundary_is_ptt() {
    let mut sm = auto_sm();
    let now = Instant::now();
    // Key down: StartRecording immediately.
    assert_eq!(sm.process(KeyAction::Down, at(now, 0)), Some(HotkeyEvent::StartRecording));
    // Tick at exactly 300 ms: commits to PTT (no new event).
    assert_eq!(sm.tick(at(now, 300)), None);
    assert_eq!(sm.process(KeyAction::Up, at(now, 300)), Some(HotkeyEvent::StopRecording));
}

#[test]
fn auto_press_just_below_threshold_is_toggle() {
    // 299 ms < 300 ms hold_delay → tick does not commit; key-up cancels to toggle.
    let mut sm = auto_sm();
    let now = Instant::now();
    assert_eq!(sm.process(KeyAction::Down, at(now, 0)), Some(HotkeyEvent::StartRecording));
    assert_eq!(sm.tick(at(now, 299)), None);
    // Key released before hold_delay → CancelAndToggle.
    assert_eq!(sm.process(KeyAction::Up, at(now, 299)), Some(HotkeyEvent::CancelAndToggle));
}

// ---------------------------------------------------------------------------
// Debounce tests (state machine level)
// ---------------------------------------------------------------------------
//
// The 50 ms debounce is enforced in HotkeyMonitor (I/O layer), not in
// HotkeyStateMachine.  We test the *monitor-level* debounce logic here by
// simulating the timestamp comparison that HotkeyMonitor performs.

/// Simulate what HotkeyMonitor does: skip events within 50 ms of the last one.
fn simulate_debounce(
    sm: &mut HotkeyStateMachine,
    action: KeyAction,
    now: Instant,
    last: &mut Option<Instant>,
) -> Option<HotkeyEvent> {
    const DEBOUNCE: Duration = Duration::from_millis(50);
    if let Some(prev) = *last {
        if now.duration_since(prev) < DEBOUNCE {
            return None; // suppressed by debounce
        }
    }
    *last = Some(now);
    sm.process(action, now)
}

#[test]
fn duplicate_events_within_50ms_ignored() {
    let mut sm = ptt();
    let now = Instant::now();
    let mut last: Option<Instant> = None;

    // First Down at t=0 — accepted.
    let ev1 = simulate_debounce(&mut sm, KeyAction::Down, at(now, 0), &mut last);
    assert_eq!(ev1, Some(HotkeyEvent::StartRecording));

    // Duplicate Down at t=30 ms — suppressed.
    let ev2 = simulate_debounce(&mut sm, KeyAction::Down, at(now, 30), &mut last);
    assert_eq!(ev2, None);

    // Another Down at t=60 ms (> 50 ms since t=0) — would reach state machine,
    // but PTT ignores Down while WaitingRelease, so returns None from state machine.
    let ev3 = simulate_debounce(&mut sm, KeyAction::Down, at(now, 60), &mut last);
    assert_eq!(ev3, None);

    // Key-up at t=200 ms — accepted, stops recording.
    let ev4 = simulate_debounce(&mut sm, KeyAction::Up, at(now, 200), &mut last);
    assert_eq!(ev4, Some(HotkeyEvent::StopRecording));

    // Duplicate Up at t=210 ms (< 50 ms after t=200 ms) — suppressed.
    let ev5 = simulate_debounce(&mut sm, KeyAction::Up, at(now, 210), &mut last);
    assert_eq!(ev5, None);
}

#[test]
fn events_after_debounce_window_are_accepted() {
    let mut sm = ptt();
    let now = Instant::now();
    let mut last: Option<Instant> = None;

    simulate_debounce(&mut sm, KeyAction::Down, at(now, 0), &mut last);
    simulate_debounce(&mut sm, KeyAction::Up, at(now, 100), &mut last);

    // Full new press/release cycle well after the debounce window.
    let ev = simulate_debounce(&mut sm, KeyAction::Down, at(now, 300), &mut last);
    assert_eq!(ev, Some(HotkeyEvent::StartRecording));
}
