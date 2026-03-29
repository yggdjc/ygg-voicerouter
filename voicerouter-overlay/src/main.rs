//! voicerouter-overlay — visual feedback overlay for voicerouter daemon.

mod controller;
mod protocol;
mod waveform;
mod window;

use gtk4::prelude::*;

use protocol::OverlayMsg;
use waveform::{BarColor, WaveMode};

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
                        ws.color.set(BarColor::RECORDING);
                        ws.mode.set(WaveMode::Level(level));
                        l.set_text("Listening...");
                        l.set_opacity(0.92);
                        w.set_visible(true);
                    }
                    OverlayMsg::Transcribing => {
                        ws.color.set(BarColor::THINKING);
                        ws.mode.set(WaveMode::Pulse);
                        l.set_text("Transcribing...");
                        l.set_opacity(0.70);
                        w.set_visible(true);
                    }
                    OverlayMsg::Result { .. } => {
                        ws.mode.set(WaveMode::Off);
                        w.set_visible(false);
                    }
                    OverlayMsg::Thinking => {
                        ws.color.set(BarColor::THINKING);
                        ws.mode.set(WaveMode::Pulse);
                        l.set_text("Thinking...");
                        l.set_opacity(0.70);
                        w.set_visible(true);
                    }
                    OverlayMsg::Speaking { .. } => {
                        ws.color.set(BarColor::SPEAKING);
                        ws.mode.set(WaveMode::Pulse);
                        l.set_text("Speaking...");
                        l.set_opacity(0.85);
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
