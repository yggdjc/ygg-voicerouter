pub mod sentence;
pub mod session;

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message, SpeakSource};
use crate::asr::AsrEngine;
use crate::audio_source::AudioChunk;
use crate::config::Config;
use crate::llm::{ChatMessage, LlmClient};
use crate::vad::{VadConfig, VadDetector, VadEvent};

use sentence::split_sentences;
use session::Session;

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Listening,
    Recording,
    Transcribing,
    Thinking,
    Speaking,
}

// ---------------------------------------------------------------------------
// ConversationActor
// ---------------------------------------------------------------------------

pub struct ConversationActor {
    config: Config,
    audio_rx: Receiver<AudioChunk>,
}

impl ConversationActor {
    #[must_use]
    pub fn new(config: Config, audio_rx: Receiver<AudioChunk>) -> Self {
        Self { config, audio_rx }
    }
}

impl Actor for ConversationActor {
    fn name(&self) -> &str {
        "conversation"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        if !self.config.conversation.enabled {
            log::info!("[conversation] disabled, entering idle loop");
            idle_loop(&inbox, &self.audio_rx);
            return;
        }

        let llm = match init_llm(&self.config) {
            Some(c) => c,
            None => {
                log::error!("[conversation] LLM init failed, entering idle loop");
                idle_loop(&inbox, &self.audio_rx);
                return;
            }
        };

        warmup_ping(&llm, self.config.conversation.llm_timeout_seconds);

        let feedback = self.config.sound.feedback;
        let mut state = State::Idle;
        let mut session: Option<Session> = None;
        let mut vad: Option<VadDetector> = None;
        let mut asr_engine: Option<AsrEngine> = None;
        let mut audio_buffer: Vec<f32> = Vec::new();
        let mut pending_sentences: usize = 0;
        let mut recording_start = Instant::now();

        log::info!("[conversation] ready");

        loop {
            let result = drain_control(
                &inbox,
                &mut state,
                &mut session,
                &mut vad,
                &mut pending_sentences,
                &outbox,
                &self.config,
            );
            if result == ControlResult::Shutdown {
                return;
            }

            match state {
                State::Idle => drain_audio(&self.audio_rx),
                State::Listening => {
                    if check_timeout(&session, &self.config) {
                        log::info!("[conversation] session timed out");
                        end_session(&outbox, feedback);
                        reset_state(
                            &mut state,
                            &mut session,
                            &mut vad,
                            &mut audio_buffer,
                        );
                        continue;
                    }
                    state = process_audio_listening(
                        &self.audio_rx,
                        vad.as_mut(),
                        &mut audio_buffer,
                        &mut recording_start,
                    );
                }
                State::Recording => {
                    state = process_audio_recording(
                        &self.audio_rx,
                        vad.as_mut(),
                        &mut audio_buffer,
                        recording_start,
                        self.config.conversation.max_turn_seconds,
                    );
                }
                State::Transcribing => {
                    state = handle_transcribing(
                        &mut asr_engine,
                        &self.config,
                        &audio_buffer,
                        &mut session,
                        &outbox,
                        &mut vad,
                    );
                    audio_buffer.clear();
                }
                State::Thinking => {
                    state = handle_thinking(
                        &llm,
                        &self.config,
                        &mut session,
                        &mut pending_sentences,
                        &outbox,
                        &mut vad,
                    );
                }
                State::Speaking => {
                    // Wait for SpeakDone messages via drain_control.
                    // Sleep briefly to avoid busy-spinning.
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers: confidence, speak, session management
// ---------------------------------------------------------------------------

fn apply_confidence(
    reply: &str,
    confidence: f64,
    high: f64,
    low: f64,
    prefix: &str,
    reject: &str,
) -> String {
    if confidence >= high {
        reply.to_string()
    } else if confidence >= low {
        format!("{prefix}{reply}")
    } else {
        reject.to_string()
    }
}

fn speak_text(text: &str, outbox: &Sender<Message>) {
    outbox
        .send(Message::SpeakRequest {
            text: text.to_string(),
            source: SpeakSource::SystemFeedback,
        })
        .ok();
}

fn speak_reply(text: &str, outbox: &Sender<Message>) {
    outbox
        .send(Message::SpeakRequest {
            text: text.to_string(),
            source: SpeakSource::LlmReply,
        })
        .ok();
}

fn end_session(outbox: &Sender<Message>, feedback: bool) {
    if feedback {
        crate::sound::beep_done().ok();
    }
    outbox.send(Message::UnmuteInput).ok();
}

fn reset_state(
    state: &mut State,
    session: &mut Option<Session>,
    vad: &mut Option<VadDetector>,
    audio_buffer: &mut Vec<f32>,
) {
    *state = State::Idle;
    *session = None;
    *vad = None;
    audio_buffer.clear();
}

// ---------------------------------------------------------------------------
// Helpers: idle loop, warmup, LLM init
// ---------------------------------------------------------------------------

fn idle_loop(inbox: &Receiver<Message>, audio_rx: &Receiver<AudioChunk>) {
    loop {
        crossbeam::select! {
            recv(inbox) -> msg => {
                match msg {
                    Ok(Message::Shutdown) => {
                        log::info!("[conversation] stopped");
                        return;
                    }
                    Err(_) => return,
                    _ => {}
                }
            }
            recv(audio_rx) -> _ => {} // discard
        }
    }
}

fn init_llm(config: &Config) -> Option<LlmClient> {
    let llm_config = crate::config::LlmConfig {
        endpoint: config.conversation.llm.endpoint.clone(),
        model: config.conversation.llm.model.clone(),
        api_key_env: String::new(),
    };
    match LlmClient::new(&llm_config) {
        Ok(c) => {
            log::info!("[conversation] LLM client ready");
            Some(c)
        }
        Err(e) => {
            log::error!("[conversation] LLM client init failed: {e:#}");
            None
        }
    }
}

fn warmup_ping(llm: &LlmClient, timeout: u64) {
    let warmup_msgs = vec![ChatMessage {
        role: "user".into(),
        content: "ping".into(),
    }];
    match llm.chat(&warmup_msgs, timeout) {
        Ok(_) => log::info!("[conversation] LLM warmup succeeded"),
        Err(e) => log::warn!("[conversation] LLM warmup failed (non-fatal): {e:#}"),
    }
}

// ---------------------------------------------------------------------------
// Helpers: control message drain
// ---------------------------------------------------------------------------

#[derive(PartialEq)]
enum ControlResult {
    Continue,
    Shutdown,
}

fn drain_control(
    inbox: &Receiver<Message>,
    state: &mut State,
    session: &mut Option<Session>,
    vad: &mut Option<VadDetector>,
    pending_sentences: &mut usize,
    outbox: &Sender<Message>,
    config: &Config,
) -> ControlResult {
    while let Ok(msg) = inbox.try_recv() {
        match msg {
            Message::Shutdown => {
                log::info!("[conversation] stopped");
                return ControlResult::Shutdown;
            }
            Message::StartConversation { wakeword } => {
                if *state == State::Idle {
                    log::info!(
                        "[conversation] starting session (wakeword={wakeword:?})"
                    );
                    start_session(state, session, vad, config, outbox);
                }
            }
            Message::EndConversation => {
                if *state != State::Idle {
                    log::info!("[conversation] ending session by request");
                    speak_text("好的，再见", outbox);
                    end_session(outbox, config.sound.feedback);
                    *state = State::Idle;
                    *session = None;
                    *vad = None;
                    *pending_sentences = 0;
                }
            }
            Message::SpeakDone => {
                if *state == State::Speaking {
                    *pending_sentences = pending_sentences.saturating_sub(1);
                    if *pending_sentences == 0 {
                        log::debug!("[conversation] all sentences spoken");
                        *state = State::Listening;
                        if let Some(ref mut s) = session {
                            s.last_activity = Instant::now();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    ControlResult::Continue
}

fn start_session(
    state: &mut State,
    session: &mut Option<Session>,
    vad: &mut Option<VadDetector>,
    config: &Config,
    outbox: &Sender<Message>,
) {
    let conv = &config.conversation;
    *session = Some(Session::new(
        conv.llm.system_prompt.clone(),
        conv.end_phrases.clone(),
    ));
    *vad = Some(VadDetector::new(&VadConfig {
        sample_rate: config.audio.sample_rate,
        threshold: config.audio.silence_threshold as f32,
    }));
    outbox.send(Message::MuteInput).ok();
    if config.sound.feedback {
        crate::sound::beep_start().ok();
    }
    *state = State::Listening;
}

// ---------------------------------------------------------------------------
// Helpers: audio processing
// ---------------------------------------------------------------------------

fn drain_audio(audio_rx: &Receiver<AudioChunk>) {
    while audio_rx.try_recv().is_ok() {}
    // Brief sleep to avoid busy-spinning in idle.
    std::thread::sleep(Duration::from_millis(50));
}

fn check_timeout(session: &Option<Session>, config: &Config) -> bool {
    session
        .as_ref()
        .map_or(false, |s| s.is_timed_out(config.conversation.timeout_seconds))
}

fn process_audio_listening(
    audio_rx: &Receiver<AudioChunk>,
    vad: Option<&mut VadDetector>,
    audio_buffer: &mut Vec<f32>,
    recording_start: &mut Instant,
) -> State {
    let Some(vad) = vad else { return State::Listening };
    match audio_rx.recv_timeout(Duration::from_millis(100)) {
        Ok(chunk) => {
            let events = vad.feed(&chunk);
            for event in events {
                match event {
                    VadEvent::Segment(segment) => {
                        audio_buffer.extend_from_slice(&segment);
                        return State::Transcribing;
                    }
                }
            }
            if vad.in_speech() {
                *recording_start = Instant::now();
                return State::Recording;
            }
            State::Listening
        }
        Err(_) => State::Listening,
    }
}

fn process_audio_recording(
    audio_rx: &Receiver<AudioChunk>,
    vad: Option<&mut VadDetector>,
    audio_buffer: &mut Vec<f32>,
    recording_start: Instant,
    max_turn_seconds: f64,
) -> State {
    let Some(vad) = vad else { return State::Listening };

    // Check max turn duration.
    if recording_start.elapsed().as_secs_f64() >= max_turn_seconds {
        log::info!("[conversation] max turn duration reached");
        return State::Transcribing;
    }

    match audio_rx.recv_timeout(Duration::from_millis(100)) {
        Ok(chunk) => {
            audio_buffer.extend_from_slice(&chunk);
            let events = vad.feed(&chunk);
            for event in events {
                match event {
                    VadEvent::Segment(segment) => {
                        // VAD completed a segment — use that instead of raw buffer.
                        audio_buffer.clear();
                        audio_buffer.extend_from_slice(&segment);
                        return State::Transcribing;
                    }
                }
            }
            State::Recording
        }
        Err(_) => State::Recording,
    }
}

// ---------------------------------------------------------------------------
// Helpers: transcribing and thinking
// ---------------------------------------------------------------------------

fn handle_transcribing(
    asr_engine: &mut Option<AsrEngine>,
    config: &Config,
    audio_buffer: &[f32],
    session: &mut Option<Session>,
    outbox: &Sender<Message>,
    vad: &mut Option<VadDetector>,
) -> State {
    // Lazy-init ASR engine.
    if asr_engine.is_none() {
        log::info!("[conversation] initialising ASR engine (lazy)");
        match AsrEngine::new(&config.asr) {
            Ok(e) => *asr_engine = Some(e),
            Err(e) => {
                log::error!("[conversation] ASR init failed: {e:#}");
                speak_text("语音识别初始化失败", outbox);
                end_session(outbox, config.sound.feedback);
                return State::Idle;
            }
        }
    }

    let transcript = match asr_engine
        .as_mut()
        .unwrap()
        .transcribe(audio_buffer, config.audio.sample_rate)
    {
        Ok(t) => t,
        Err(e) => {
            log::error!("[conversation] transcription failed: {e:#}");
            return State::Listening;
        }
    };

    if transcript.is_empty() {
        log::debug!("[conversation] empty transcript, back to listening");
        return State::Listening;
    }

    log::info!("[conversation] transcript: {transcript:?}");

    return finalize_transcript(
        &transcript, config, session, outbox, vad,
    );
}

fn finalize_transcript(
    transcript: &str,
    config: &Config,
    session: &mut Option<Session>,
    outbox: &Sender<Message>,
    vad: &mut Option<VadDetector>,
) -> State {
    let Some(ref mut sess) = session else {
        return State::Idle;
    };

    if sess.is_end_phrase(transcript) {
        log::info!("[conversation] end phrase detected");
        speak_text("好的，再见", outbox);
        end_session(outbox, config.sound.feedback);
        *session = None;
        *vad = None;
        return State::Idle;
    }

    sess.add_user_message(transcript);
    State::Thinking
}

fn handle_thinking(
    llm: &LlmClient,
    config: &Config,
    session: &mut Option<Session>,
    pending_sentences: &mut usize,
    outbox: &Sender<Message>,
    vad: &mut Option<VadDetector>,
) -> State {
    let Some(ref mut sess) = session else {
        return State::Idle;
    };

    let messages = sess.messages();
    let timeout = config.conversation.llm_timeout_seconds;

    let response = llm
        .chat(&messages, timeout)
        .or_else(|e| {
            log::warn!("[conversation] LLM first attempt failed: {e:#}, retrying");
            llm.chat(&messages, timeout)
        });

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            log::error!("[conversation] LLM failed after retry: {e:#}");
            speak_text("抱歉，我暂时无法回答", outbox);
            end_session(outbox, config.sound.feedback);
            *session = None;
            *vad = None;
            return State::Idle;
        }
    };

    let conv = &config.conversation;
    let reply = apply_confidence(
        &response.reply,
        response.confidence,
        conv.confidence_high,
        conv.confidence_low,
        &conv.low_confidence_prefix,
        &conv.low_confidence_reject,
    );

    log::info!(
        "[conversation] reply (confidence={:.2}): {reply:?}",
        response.confidence
    );

    sess.add_assistant_message(&reply);

    let sentences = split_sentences(&reply);
    if sentences.is_empty() {
        return State::Listening;
    }

    *pending_sentences = sentences.len();
    for sentence in &sentences {
        speak_reply(sentence, outbox);
    }

    State::Speaking
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_high_returns_reply() {
        assert_eq!(
            apply_confidence("你好", 0.9, 0.8, 0.5, "前缀", "拒绝"),
            "你好"
        );
    }

    #[test]
    fn confidence_medium_adds_prefix() {
        assert_eq!(
            apply_confidence("你好", 0.6, 0.8, 0.5, "我不确定，", "拒绝"),
            "我不确定，你好"
        );
    }

    #[test]
    fn confidence_low_returns_reject() {
        assert_eq!(
            apply_confidence("你好", 0.3, 0.8, 0.5, "前缀", "无法回答"),
            "无法回答"
        );
    }

    #[test]
    fn confidence_at_boundary_high() {
        assert_eq!(apply_confidence("ok", 0.8, 0.8, 0.5, "pfx", "rej"), "ok");
    }

    #[test]
    fn confidence_at_boundary_low() {
        assert_eq!(
            apply_confidence("ok", 0.5, 0.8, 0.5, "pfx:", "rej"),
            "pfx:ok"
        );
    }

    #[test]
    fn conversation_actor_name() {
        let (_tx, rx) = crossbeam::channel::bounded(1);
        let actor = ConversationActor::new(Config::default(), rx);
        assert_eq!(Actor::name(&actor), "conversation");
    }
}
