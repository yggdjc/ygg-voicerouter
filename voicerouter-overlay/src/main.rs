//! voicerouter-overlay — visual feedback overlay for voicerouter daemon.

use gtk4::prelude::*;

mod protocol;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    log::info!("voicerouter-overlay starting");

    let app = gtk4::Application::builder()
        .application_id("com.voicerouter.overlay")
        .build();

    app.connect_activate(|_app| {
        log::info!("GTK activated (window creation in later task)");
    });

    app.run();
}
