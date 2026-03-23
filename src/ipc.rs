//! IPC actor — Unix socket server with JSON-RPC 2.0 protocol.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::actor::{Actor, Message, Metadata};
use crate::config::IpcConfig;

// ---- JSON-RPC types ----

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize, Debug)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Serialize, Debug)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn ok(result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", result: Some(result), error: None }
    }

    pub fn error(code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError { code, message: message.into() }),
        }
    }
}

/// Format a push event notification.
pub fn json_event(
    event_type: &str,
    text: &str,
    raw: Option<&str>,
) -> serde_json::Value {
    let mut params = serde_json::Map::new();
    params.insert("type".into(), event_type.into());
    params.insert("text".into(), text.into());
    if let Some(r) = raw {
        params.insert("raw".into(), r.into());
    }
    serde_json::json!({ "method": "event", "params": params })
}

/// Resolve socket path: use custom if non-empty, else XDG_RUNTIME_DIR.
pub fn resolve_socket_path(configured: &str) -> String {
    if !configured.is_empty() {
        return configured.to_string();
    }
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{runtime_dir}/voicerouter.sock")
    } else {
        "/tmp/voicerouter.sock".to_string()
    }
}

// ---- IpcActor ----

pub struct IpcActor {
    config: IpcConfig,
}

impl IpcActor {
    #[must_use]
    pub fn new(config: IpcConfig) -> Self {
        Self { config }
    }
}

impl Actor for IpcActor {
    fn name(&self) -> &str {
        "ipc"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let socket_path = resolve_socket_path(&self.config.socket_path);

        // Remove stale socket file.
        let _ = std::fs::remove_file(&socket_path);

        let listener = match UnixListener::bind(&socket_path) {
            Ok(l) => l,
            Err(e) => {
                log::error!("[ipc] failed to bind {socket_path}: {e}");
                return;
            }
        };

        // Set socket permissions to 0600.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            &socket_path,
            std::fs::Permissions::from_mode(0o600),
        );

        listener.set_nonblocking(true).ok();
        log::info!("[ipc] listening on {socket_path}");

        let clients: Arc<Mutex<Vec<Arc<Mutex<UnixStream>>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let max_conn = self.config.max_connections;

        loop {
            // Check for shutdown or bus events.
            let mut shutdown = false;
            while let Ok(msg) = inbox.try_recv() {
                match msg {
                    Message::Shutdown => {
                        log::info!("[ipc] shutting down");
                        let _ = std::fs::remove_file(&socket_path);
                        shutdown = true;
                        break;
                    }
                    Message::Transcript { ref text, ref raw } => {
                        let event = json_event("transcript", text, Some(raw));
                        push_to_clients(&clients, &event);
                    }
                    Message::PipelineOutput { ref text, ref stage } => {
                        let event =
                            json_event("pipeline_output", text, Some(stage));
                        push_to_clients(&clients, &event);
                    }
                    _ => {}
                }
            }
            if shutdown {
                break;
            }

            // Accept new connections.
            if let Ok((stream, _addr)) = listener.accept() {
                let mut client_list = clients.lock().unwrap();
                if client_list.len() >= max_conn {
                    log::warn!("[ipc] max connections reached, rejecting");
                    let mut s = stream;
                    let resp = JsonRpcResponse::error(
                        -32000,
                        "max connections reached",
                    );
                    let _ = writeln!(
                        s,
                        "{}",
                        serde_json::to_string(&resp).unwrap()
                    );
                    drop(s);
                } else {
                    let client = Arc::new(Mutex::new(stream));
                    client_list.push(Arc::clone(&client));
                    let outbox_clone = outbox.clone();
                    std::thread::spawn(move || {
                        handle_client(client, outbox_clone);
                    });
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

fn handle_client(stream: Arc<Mutex<UnixStream>>, outbox: Sender<Message>) {
    let reader_stream = stream.lock().unwrap().try_clone().unwrap();
    let reader = BufReader::new(reader_stream);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.len() > 65536 {
            let resp = JsonRpcResponse::error(-32000, "message too large");
            if let Ok(mut s) = stream.lock() {
                let _ = writeln!(
                    s,
                    "{}",
                    serde_json::to_string(&resp).unwrap()
                );
            }
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => {
                let resp = JsonRpcResponse::error(-32700, "Parse error");
                if let Ok(mut s) = stream.lock() {
                    let _ = writeln!(
                        s,
                        "{}",
                        serde_json::to_string(&resp).unwrap()
                    );
                }
                continue;
            }
        };

        let resp = handle_request(&req, &outbox);
        if let Ok(mut s) = stream.lock() {
            let _ =
                writeln!(s, "{}", serde_json::to_string(&resp).unwrap());
        }
    }
}

fn handle_request(
    req: &JsonRpcRequest,
    outbox: &Sender<Message>,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "pipeline.send" => {
            let text = req
                .params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return JsonRpcResponse::error(
                    -32602,
                    "missing 'text' parameter",
                );
            }
            let msg = Message::PipelineInput {
                text: text.to_string(),
                metadata: Metadata {
                    source: "ipc".to_string(),
                    timestamp: Instant::now(),
                },
            };
            outbox.send(msg).ok();
            JsonRpcResponse::ok(serde_json::json!({"status": "ok"}))
        }
        "recording.start" => {
            outbox.send(Message::StartListening { wakeword: None }).ok();
            JsonRpcResponse::ok(serde_json::json!({"status": "ok"}))
        }
        "recording.stop" => {
            outbox.send(Message::StopListening).ok();
            JsonRpcResponse::ok(serde_json::json!({"status": "ok"}))
        }
        "status" => {
            JsonRpcResponse::ok(serde_json::json!({"status": "running"}))
        }
        "events.subscribe" => {
            // Subscription is implicit — all connected clients receive events.
            JsonRpcResponse::ok(serde_json::json!({"status": "subscribed"}))
        }
        _ => JsonRpcResponse::error(
            -32601,
            &format!("unknown method: {}", req.method),
        ),
    }
}

fn push_to_clients(
    clients: &Arc<Mutex<Vec<Arc<Mutex<UnixStream>>>>>,
    event: &serde_json::Value,
) {
    let json = serde_json::to_string(event).unwrap();
    let mut client_list = clients.lock().unwrap();
    client_list.retain(|client| {
        if let Ok(mut s) = client.lock() {
            writeln!(s, "{json}").is_ok()
        } else {
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pipeline_send() {
        let json =
            r#"{"method":"pipeline.send","params":{"text":"hello"}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "pipeline.send");
        assert_eq!(req.params["text"].as_str().unwrap(), "hello");
    }

    #[test]
    fn parse_recording_start() {
        let json = r#"{"method":"recording.start"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "recording.start");
    }

    #[test]
    fn parse_events_subscribe() {
        let json = r#"{"method":"events.subscribe","params":{"types":["transcript"]}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        let types = req.params["types"].as_array().unwrap();
        assert_eq!(types[0].as_str().unwrap(), "transcript");
    }

    #[test]
    fn format_error_response() {
        let resp = JsonRpcResponse::error(-32700, "Parse error");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32700"));
        assert!(json.contains("Parse error"));
    }

    #[test]
    fn format_event_notification() {
        let event = json_event("transcript", "\u{4f60}\u{597d}\u{4e16}\u{754c}", Some("\u{4f60}\u{597d}\u{4e16}\u{754c}"));
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("transcript"));
        assert!(json.contains("\u{4f60}\u{597d}\u{4e16}\u{754c}"));
    }

    #[test]
    fn default_socket_path_uses_xdg() {
        let path = resolve_socket_path("");
        assert!(path.ends_with("voicerouter.sock"));
    }

    #[test]
    fn custom_socket_path_is_used() {
        let path = resolve_socket_path("/tmp/custom.sock");
        assert_eq!(path, "/tmp/custom.sock");
    }
}
