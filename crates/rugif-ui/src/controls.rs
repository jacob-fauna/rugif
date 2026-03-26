use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;

/// Shows a small floating controls window. Returns when the user clicks Stop.
pub fn show_recording_controls(
    stop_flag: Arc<AtomicBool>,
    region_x: i32,
    region_y: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = crate::native_options_any_thread(
        egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_inner_size([220.0, 70.0])
            .with_position([region_x as f32, ((region_y - 80) as f32).max(0.0)]),
    );

    eframe::run_native(
        "rugif - Recording",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ControlsApp {
                stop_flag,
                start_time: Instant::now(),
            }))
        }),
    )?;

    Ok(())
}

struct ControlsApp {
    stop_flag: Arc<AtomicBool>,
    start_time: Instant,
}

impl eframe::App for ControlsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let elapsed = self.start_time.elapsed();
        let secs = elapsed.as_secs();
        let time_str = format!("{:02}:{:02}", secs / 60, secs % 60);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgba_unmultiplied(30, 30, 30, 230))
                    .inner_margin(12.0)
                    .corner_radius(8.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Recording indicator (red dot)
                    let dot_rect = egui::Rect::from_center_size(
                        ui.cursor().min + egui::vec2(6.0, 10.0),
                        egui::vec2(12.0, 12.0),
                    );
                    ui.painter()
                        .circle_filled(dot_rect.center(), 6.0, egui::Color32::RED);
                    ui.add_space(16.0);

                    // Timer
                    ui.label(
                        egui::RichText::new(&time_str)
                            .color(egui::Color32::WHITE)
                            .size(18.0)
                            .monospace(),
                    );

                    ui.add_space(12.0);

                    // Stop button
                    let stop_btn = ui.add(
                        egui::Button::new(
                            egui::RichText::new("  Stop  ")
                                .color(egui::Color32::WHITE)
                                .size(14.0),
                        )
                        .fill(egui::Color32::from_rgb(200, 40, 40))
                        .corner_radius(4.0),
                    );

                    if stop_btn.clicked() {
                        self.stop_flag.store(true, Ordering::Relaxed);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });

        // Handle Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stop_flag.store(true, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Repaint to update the timer
        ctx.request_repaint();
    }
}
