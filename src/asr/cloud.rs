//! Cloud ASR via DashScope Qwen3-ASR-Flash-Realtime WebSocket API.
//!
//! Sends complete audio buffers (manual mode, no server VAD) and receives
//! transcription results. Falls back gracefully on connection failure.

use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine as _;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::config::AsrCloudConfig;

/// Chunk size in bytes for audio transmission (~100ms at 16kHz 16-bit mono).
const AUDIO_CHUNK_BYTES: usize = 3200;

/// Maximum time to wait for transcription result.
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

/// Non-blocking poll timeout for checking partial results.
const POLL_TIMEOUT: Duration = Duration::from_millis(1);

pub struct CloudAsr {
    config: AsrCloudConfig,
    api_key: String,
    ws: Option<WebSocket<MaybeTlsStream<TcpStream>>>,
    /// Whether a streaming session is currently active (between start/finish).
    streaming: bool,
}

impl CloudAsr {
    pub fn new(config: &AsrCloudConfig) -> Result<Self> {
        let api_key = if config.api_key_env.is_empty() {
            String::new()
        } else {
            std::env::var(&config.api_key_env)
                .with_context(|| format!("missing env var: {}", config.api_key_env))?
        };
        Ok(Self {
            config: config.clone(),
            api_key,
            ws: None,
            streaming: false,
        })
    }

    /// Transcribe f32 PCM audio via the cloud API.
    /// Returns the recognized text, or an error if the cloud call fails.
    /// Automatically reconnects and retries once on stale connection errors.
    pub fn transcribe(&mut self, audio: &[f32], sample_rate: u32) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        match self.transcribe_inner(audio, sample_rate) {
            Ok(text) => Ok(text),
            Err(e) => {
                log::warn!("[cloud_asr] first attempt failed: {e:#}, reconnecting");
                self.disconnect();
                self.transcribe_inner(audio, sample_rate)
            }
        }
    }

    /// Start a streaming session. Must call before `send_audio`.
    /// Ensures the WebSocket is connected and configured.
    pub fn start_stream(&mut self, sample_rate: u32) -> Result<()> {
        // Tear down any stale connection first.
        if self.streaming {
            log::warn!("[cloud_asr] start_stream called while already streaming");
            self.disconnect();
            self.streaming = false;
        }

        if self.ws.is_none() {
            self.connect()?;
            self.send_session_update(sample_rate)?;
        }
        self.streaming = true;
        log::debug!("[cloud_asr] stream started");
        Ok(())
    }

    /// Send an audio chunk during recording. Non-blocking (fire-and-forget).
    pub fn send_audio(&mut self, audio: &[f32]) -> Result<()> {
        if audio.is_empty() {
            return Ok(());
        }
        let pcm_bytes = f32_to_i16_bytes(audio);
        for chunk in pcm_bytes.chunks(AUDIO_CHUNK_BYTES) {
            let b64 = base64::engine::general_purpose::STANDARD.encode(chunk);
            let event = serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": b64
            });
            if let Err(e) = self.send_json(&event) {
                self.disconnect();
                self.streaming = false;
                return Err(e.context("send_audio: failed to send chunk"));
            }
        }
        Ok(())
    }

    /// Non-blocking poll for partial transcription results.
    /// Returns the latest partial text seen, or `None` if nothing available.
    pub fn poll_partial(&mut self) -> Option<String> {
        let ws = self.ws.as_mut()?;

        // Set very short timeout for non-blocking read.
        set_read_timeout(ws, Some(POLL_TIMEOUT));

        let mut latest: Option<String> = None;
        loop {
            match ws.read() {
                Ok(Message::Text(text)) => {
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(
                        text.as_str(),
                    ) {
                        let msg_type = data["type"].as_str().unwrap_or("");
                        if msg_type
                            == "conversation.item.input_audio_transcription.text"
                        {
                            if let Some(t) = data["text"].as_str() {
                                let trimmed = t.trim();
                                if !trimmed.is_empty() {
                                    latest = Some(trimmed.to_string());
                                }
                            }
                        }
                        // Ignore other events during polling.
                    }
                }
                Ok(Message::Close(_)) => {
                    log::warn!("[cloud_asr] connection closed during poll");
                    self.disconnect();
                    self.streaming = false;
                    break;
                }
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // No more data available right now.
                    break;
                }
                Err(_) => {
                    // Any other read error — stop polling this tick.
                    break;
                }
                _ => {} // Ping/Pong
            }
        }

        // Restore normal timeout for subsequent blocking reads.
        if let Some(ws) = self.ws.as_ref() {
            set_read_timeout(ws, Some(RECV_TIMEOUT));
        }

        latest
    }

    /// Finish the streaming session: send commit, wait for `.completed`.
    pub fn finish_stream(&mut self) -> Result<String> {
        if !self.streaming {
            anyhow::bail!("finish_stream called without active stream");
        }
        self.streaming = false;

        let commit = serde_json::json!({"type": "input_audio_buffer.commit"});
        if let Err(e) = self.send_json(&commit) {
            self.disconnect();
            return Err(e.context("finish_stream: failed to send commit"));
        }

        self.recv_transcript()
    }

    fn transcribe_inner(&mut self, audio: &[f32], sample_rate: u32) -> Result<String> {
        // Ensure connection.
        if self.ws.is_none() {
            self.connect()?;
            self.send_session_update(sample_rate)?;
        }

        // Convert f32 to i16 PCM bytes.
        let pcm_bytes = f32_to_i16_bytes(audio);

        // Send audio in chunks.
        for chunk in pcm_bytes.chunks(AUDIO_CHUNK_BYTES) {
            let b64 = base64::engine::general_purpose::STANDARD.encode(chunk);
            let event = serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": b64
            });
            if let Err(e) = self.send_json(&event) {
                self.disconnect();
                return Err(e.context("failed to send audio chunk"));
            }
        }

        // Commit to trigger transcription.
        let commit = serde_json::json!({"type": "input_audio_buffer.commit"});
        if let Err(e) = self.send_json(&commit) {
            self.disconnect();
            return Err(e.context("failed to send commit"));
        }

        // Wait for completed transcription.
        self.recv_transcript()
    }

    fn connect(&mut self) -> Result<()> {
        let url = format!("{}?model={}", self.config.endpoint, self.config.model);
        let mut request = url
            .into_client_request()
            .context("invalid WebSocket URL")?;
        let headers = request.headers_mut();
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.api_key)
                .parse()
                .context("invalid Authorization header")?,
        );
        headers.insert("OpenAI-Beta", "realtime=v1".parse().unwrap());

        let (ws, _response) =
            tungstenite::connect(request).context("WebSocket connection failed")?;
        self.ws = Some(ws);

        // Read session.created.
        self.recv_event("session.created")?;
        log::info!("[cloud_asr] connected");
        Ok(())
    }

    fn disconnect(&mut self) {
        if let Some(mut ws) = self.ws.take() {
            let _ = ws.close(None);
        }
    }

    fn send_session_update(&mut self, sample_rate: u32) -> Result<()> {
        let event = serde_json::json!({
            "type": "session.update",
            "session": {
                "modalities": ["text"],
                "input_audio_format": "pcm",
                "sample_rate": sample_rate,
                "input_audio_transcription": {
                    "language": self.config.language
                },
                "turn_detection": null
            }
        });
        self.send_json(&event)?;
        // Read session.updated confirmation.
        self.recv_event("session.updated")?;
        log::debug!("[cloud_asr] session configured");
        Ok(())
    }

    fn send_json(&mut self, value: &serde_json::Value) -> Result<()> {
        let ws = self.ws.as_mut().context("not connected")?;
        let text = serde_json::to_string(value)?;
        ws.send(Message::Text(text.into()))
            .context("WebSocket send failed")
    }

    fn recv_transcript(&mut self) -> Result<String> {
        let ws = self.ws.as_mut().context("not connected")?;
        set_read_timeout(ws, Some(RECV_TIMEOUT));

        loop {
            let msg = ws.read().context("WebSocket read failed")?;
            match msg {
                Message::Text(text) => {
                    let data: serde_json::Value = serde_json::from_str(text.as_str())
                        .context("invalid JSON from server")?;
                    let msg_type = data["type"].as_str().unwrap_or("");

                    match msg_type {
                        "conversation.item.input_audio_transcription.completed" => {
                            let transcript = data["transcript"]
                                .as_str()
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            log::info!("[cloud_asr] transcript: {transcript:?}");
                            return Ok(transcript);
                        }
                        "error" => {
                            let err_msg = data["error"]["message"]
                                .as_str()
                                .unwrap_or("unknown error");
                            self.disconnect();
                            anyhow::bail!("[cloud_asr] API error: {err_msg}");
                        }
                        _ => {
                            log::debug!("[cloud_asr] event: {msg_type}");
                        }
                    }
                }
                Message::Close(_) => {
                    self.disconnect();
                    anyhow::bail!("[cloud_asr] connection closed by server");
                }
                _ => {} // Ping/Pong handled by tungstenite
            }
        }
    }

    /// Wait for a specific event type. Returns the event data.
    fn recv_event(&mut self, expected_type: &str) -> Result<serde_json::Value> {
        let ws = self.ws.as_mut().context("not connected")?;
        set_read_timeout(ws, Some(Duration::from_secs(5)));

        loop {
            let msg = ws.read().context("WebSocket read failed")?;
            if let Message::Text(text) = msg {
                let data: serde_json::Value = serde_json::from_str(text.as_str())?;
                let msg_type = data["type"].as_str().unwrap_or("");
                if msg_type == expected_type {
                    return Ok(data);
                }
                if msg_type == "error" {
                    let err = data["error"]["message"].as_str().unwrap_or("unknown");
                    anyhow::bail!(
                        "[cloud_asr] error while waiting for {expected_type}: {err}"
                    );
                }
                log::debug!(
                    "[cloud_asr] skipping event: {msg_type} (waiting for {expected_type})"
                );
            }
        }
    }
}

/// Set read timeout on the underlying TCP stream, regardless of TLS wrapping.
fn set_read_timeout(ws: &WebSocket<MaybeTlsStream<TcpStream>>, timeout: Option<Duration>) {
    match ws.get_ref() {
        MaybeTlsStream::Plain(ref tcp) => {
            tcp.set_read_timeout(timeout).ok();
        }
        MaybeTlsStream::NativeTls(ref tls) => {
            tls.get_ref().set_read_timeout(timeout).ok();
        }
        _ => {}
    }
}

/// Convert f32 audio samples to i16 PCM bytes (little-endian).
fn f32_to_i16_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        bytes.extend_from_slice(&i16_val.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_to_i16_conversion() {
        let samples = vec![0.0, 1.0, -1.0, 0.5];
        let bytes = f32_to_i16_bytes(&samples);
        assert_eq!(bytes.len(), 8); // 4 samples * 2 bytes

        // 0.0 -> 0
        assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), 0);
        // 1.0 -> 32767
        assert_eq!(i16::from_le_bytes([bytes[2], bytes[3]]), 32767);
        // -1.0 -> -32767
        assert_eq!(i16::from_le_bytes([bytes[4], bytes[5]]), -32767);
    }

    #[test]
    fn cloud_asr_new_missing_env() {
        let config = AsrCloudConfig {
            enabled: true,
            endpoint: "wss://example.com".into(),
            model: "test".into(),
            api_key_env: "NONEXISTENT_KEY_FOR_TEST_12345".into(),
            language: "zh".into(),
        };
        let result = CloudAsr::new(&config);
        assert!(result.is_err());
    }
}
