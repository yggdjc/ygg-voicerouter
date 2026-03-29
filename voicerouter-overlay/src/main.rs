//! voicerouter-overlay — visual feedback overlay for voicerouter daemon.

mod protocol;
mod waveform;
mod window;

use gtk4::prelude::*;

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
        let (window, _label, _wave_state) = window::build_window(app);
        window.set_visible(true);
        log::info!("overlay window created");
    });

    app.run();
}
