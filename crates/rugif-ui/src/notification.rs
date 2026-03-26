use eframe::egui;
use std::path::Path;

/// Show a small notification window with the save path. Auto-closes after a few seconds.
pub fn show_save_notification(path: &Path) {
    let path_str = path.display().to_string();

    let options = crate::native_options_any_thread(
        egui::ViewportBuilder::default()
            .with_decorations(true)
            .with_inner_size([400.0, 120.0])
            .with_always_on_top()
            .with_title("rugif"),
    );

    let _ = eframe::run_native(
        "rugif - Saved",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(NotificationApp {
                path: path_str,
                start: std::time::Instant::now(),
            }))
        }),
    );
}

struct NotificationApp {
    path: String,
    start: std::time::Instant,
}

impl eframe::App for NotificationApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-close after 8 seconds.
        if self.start.elapsed().as_secs() >= 8 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("GIF saved!")
                        .size(18.0)
                        .color(egui::Color32::from_rgb(80, 200, 80)),
                );
            });

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(&self.path);
            });

            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.add_space(10.0);
                if ui.button("Open folder").clicked() {
                    if let Some(parent) = std::path::Path::new(&self.path).parent() {
                        let _ = std::process::Command::new("xdg-open")
                            .arg(parent)
                            .spawn();
                    }
                }
                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });

        ctx.request_repaint();
    }
}
