//! Tests for CoreActor recording stop behaviour.
//!
//! Wakeword-triggered recordings use silence auto-stop (no timeout).
//! Hotkey-triggered recordings use timeout only (no silence auto-stop).

use std::time::{Duration, Instant};

use voicerouter::core_actor::{RecordingStopCheck, StopReason};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn wakeword_check() -> RecordingStopCheck {
    RecordingStopCheck {
        is_wakeword: true,
        speech_detected: true,
        silence_duration: Duration::from_secs_f32(1.5),
        max_record: Duration::from_secs(30),
    }
}

fn hotkey_check() -> RecordingStopCheck {
    RecordingStopCheck {
        is_wakeword: false,
        speech_detected: true,
        silence_duration: Duration::from_secs_f32(1.5),
        max_record: Duration::from_secs(60),
    }
}

// ---------------------------------------------------------------------------
// Wakeword mode: silence auto-stop
// ---------------------------------------------------------------------------

#[test]
fn wakeword_stops_on_silence() {
    let check = wakeword_check();
    let silence_since = Some(Instant::now() - Duration::from_secs(2));
    let recording_start = Instant::now() - Duration::from_secs(5);

    assert_eq!(
        check.should_stop(silence_since, recording_start),
        Some(StopReason::Silence),
    );
}

#[test]
fn wakeword_does_not_stop_on_brief_silence() {
    let check = wakeword_check();
    let silence_since = Some(Instant::now() - Duration::from_millis(500));
    let recording_start = Instant::now() - Duration::from_secs(5);

    assert_eq!(check.should_stop(silence_since, recording_start), None);
}

#[test]
fn wakeword_does_not_stop_before_speech() {
    let mut check = wakeword_check();
    check.speech_detected = false;
    let silence_since = Some(Instant::now() - Duration::from_secs(3));
    let recording_start = Instant::now() - Duration::from_secs(5);

    assert_eq!(check.should_stop(silence_since, recording_start), None);
}

#[test]
fn wakeword_ignores_timeout() {
    let check = wakeword_check();
    // 60s elapsed but no silence — should NOT stop.
    let recording_start = Instant::now() - Duration::from_secs(60);

    assert_eq!(check.should_stop(None, recording_start), None);
}

// ---------------------------------------------------------------------------
// Hotkey mode: timeout only, no silence auto-stop
// ---------------------------------------------------------------------------

#[test]
fn hotkey_does_not_stop_on_silence() {
    let check = hotkey_check();
    // Long silence, but hotkey mode ignores it.
    let silence_since = Some(Instant::now() - Duration::from_secs(10));
    let recording_start = Instant::now() - Duration::from_secs(5);

    assert_eq!(check.should_stop(silence_since, recording_start), None);
}

#[test]
fn hotkey_stops_on_timeout() {
    let check = hotkey_check();
    let recording_start = Instant::now() - Duration::from_secs(61);

    assert_eq!(
        check.should_stop(None, recording_start),
        Some(StopReason::Timeout),
    );
}

#[test]
fn hotkey_does_not_stop_before_timeout() {
    let check = hotkey_check();
    let recording_start = Instant::now() - Duration::from_secs(10);

    assert_eq!(check.should_stop(None, recording_start), None);
}
