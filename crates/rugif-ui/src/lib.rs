pub mod controls;
pub mod notification;
pub mod selection;
pub mod settings;
pub mod trim;

/// Create NativeOptions that allow running the event loop from any thread.
/// Required when launching egui windows from tray callbacks or worker threads.
pub fn native_options_any_thread(viewport: eframe::egui::ViewportBuilder) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport,
        event_loop_builder: Some(Box::new(|builder| {
            // Both X11 and Wayland traits set the same `any_thread` field.
            // Use whichever trait is available — they're both implemented.
            use winit::platform::x11::EventLoopBuilderExtX11 as _;
            builder.with_any_thread(true);
        })),
        ..Default::default()
    }
}
