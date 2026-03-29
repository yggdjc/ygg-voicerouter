//! voicerouter-overlay — visual feedback overlay for voicerouter daemon.

mod controller;
mod protocol;
mod waveform;
mod window;

use gtk4::prelude::*;

use protocol::OverlayMsg;
use waveform::WaveMode;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    log::info!("voicerouter-overlay starting");

    let app = gtk4::Application::builder()
        .application_id("com.voicerouter.overlay")
        .build();

    app.connect_activate(|app| {
        let (window, label, wave_state) = window::build_window(app);

        let rx = controller::start_listener();

        let w = window.clone();
        let l = label.clone();
        let ws = wave_state.clone();

        gtk4::glib::spawn_future_local(async move {
            while let Ok(msg) = rx.recv().await {
                match msg {
                    OverlayMsg::Recording { level } => {
                        ws.mode.set(WaveMode::Level(level));
                        l.set_text("Listening...");
                        l.set_opacity(0.9);
                        w.set_visible(true);
                    }
                    OverlayMsg::Transcribing => {
                        ws.mode.set(WaveMode::Pulse);
                        l.set_text("Transcribing...");
                        l.set_opacity(0.7);
                        w.set_visible(true);
                    }
                    OverlayMsg::Result { .. } => {
                        // Dismiss immediately — no need to show the result text.
                        ws.mode.set(WaveMode::Off);
                        w.set_visible(false);
                    }
                    OverlayMsg::Thinking => {
                        ws.mode.set(WaveMode::Pulse);
                        l.set_text("Thinking...");
                        l.set_opacity(0.7);
                        w.set_visible(true);
                    }
                    OverlayMsg::Idle => {
                        ws.mode.set(WaveMode::Off);
                        w.set_visible(false);
                    }
                }
            }
        });
    });

    app.connect_shutdown(|_| {
        controller::cleanup();
    });

    app.run();
}
