//! Socket listener and overlay state controller.

use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use crate::protocol::OverlayMsg;

pub fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    PathBuf::from(runtime_dir).join("voicerouter-overlay.sock")
}

pub fn start_listener() -> async_channel::Receiver<OverlayMsg> {
    let (tx, rx) = async_channel::unbounded();

    std::thread::Builder::new()
        .name("socket-listener".into())
        .spawn(move || {
            let path = socket_path();
            let _ = std::fs::remove_file(&path);

            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    log::error!("failed to bind socket at {}: {e}", path.display());
                    return;
                }
            };
            log::info!("listening on {}", path.display());

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let tx = tx.clone();
                        std::thread::Builder::new()
                            .name("socket-client".into())
                            .spawn(move || {
                                let reader = std::io::BufReader::new(stream);
                                for line in reader.lines() {
                                    match line {
                                        Ok(ref l) if l.trim().is_empty() => continue,
                                        Ok(l) => match serde_json::from_str::<OverlayMsg>(&l) {
                                            Ok(msg) => {
                                                if tx.send_blocking(msg).is_err() {
                                                    return;
                                                }
                                            }
                                            Err(e) => log::warn!("bad message: {e}: {l}"),
                                        },
                                        Err(e) => {
                                            log::debug!("client disconnected: {e}");
                                            return;
                                        }
                                    }
                                }
                            })
                            .ok();
                    }
                    Err(e) => log::warn!("accept error: {e}"),
                }
            }
        })
        .expect("failed to spawn socket listener thread");

    rx
}

pub fn cleanup() {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
}
