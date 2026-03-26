use eframe::egui;
use rugif_core::config::Settings;

/// Launch the settings window. Blocks until closed. Returns the updated settings.
pub fn show_settings(settings: Settings) -> Result<Settings, Box<dyn std::error::Error>> {
    let result = std::sync::Arc::new(std::sync::Mutex::new(settings.clone()));
    let result_clone = result.clone();

    let options = crate::native_options_any_thread(
        egui::ViewportBuilder::default()
            .with_inner_size([450.0, 500.0])
            .with_title("rugif Settings"),
    );

    eframe::run_native(
        "rugif Settings",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(SettingsApp {
                settings: settings.clone(),
                result: result_clone,
                status_msg: None,
            }))
        }),
    )?;

    let lock = result.lock().unwrap();
    Ok(lock.clone())
}

struct SettingsApp {
    settings: Settings,
    result: std::sync::Arc<std::sync::Mutex<Settings>>,
    status_msg: Option<(String, std::time::Instant)>,
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rugif Settings");
            ui.separator();

            // Recording section
            ui.collapsing("Recording", |ui| {
                ui.horizontal(|ui| {
                    ui.label("FPS:");
                    ui.add(egui::Slider::new(&mut self.settings.recording.fps, 5..=30));
                });

                ui.horizontal(|ui| {
                    ui.label("Quality:");
                    ui.add(egui::Slider::new(&mut self.settings.recording.quality, 1..=100));
                });

                ui.horizontal(|ui| {
                    ui.label("Max duration (sec):");
                    ui.add(egui::Slider::new(
                        &mut self.settings.recording.max_duration_secs,
                        5..=120,
                    ));
                });

                ui.horizontal(|ui| {
                    ui.label("Save directory:");
                    let path_str = self.settings.recording.save_directory.to_string_lossy();
                    let mut path_edit = path_str.to_string();
                    if ui.text_edit_singleline(&mut path_edit).changed() {
                        self.settings.recording.save_directory = path_edit.into();
                    }
                });
            });

            ui.add_space(8.0);

            // Shortcuts section
            ui.collapsing("Shortcuts", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Record:");
                    ui.text_edit_singleline(&mut self.settings.shortcuts.record);
                });

                ui.horizontal(|ui| {
                    ui.label("Stop:");
                    ui.text_edit_singleline(&mut self.settings.shortcuts.stop);
                });

                ui.label(
                    egui::RichText::new("Format: Modifier+Key (e.g. Super+Shift+R)")
                        .small()
                        .color(egui::Color32::GRAY),
                );
            });

            ui.add_space(8.0);

            // General section
            ui.collapsing("General", |ui| {
                ui.checkbox(
                    &mut self.settings.general.start_on_login,
                    "Start on login",
                );
                ui.checkbox(
                    &mut self.settings.general.start_minimized,
                    "Start minimized to tray",
                );
                ui.checkbox(
                    &mut self.settings.general.notify_on_save,
                    "Show notification when GIF is saved",
                );
            });

            ui.add_space(16.0);
            ui.separator();

            // Save / Cancel buttons
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    match self.settings.save() {
                        Ok(()) => {
                            *self.result.lock().unwrap() = self.settings.clone();
                            // Update autostart based on setting.
                            if let Err(e) = rugif_core::config::set_autostart(
                                self.settings.general.start_on_login,
                            ) {
                                self.status_msg = Some((
                                    format!("Saved, but autostart failed: {e}"),
                                    std::time::Instant::now(),
                                ));
                            } else {
                                self.status_msg =
                                    Some(("Settings saved!".into(), std::time::Instant::now()));
                            }
                        }
                        Err(e) => {
                            self.status_msg = Some((
                                format!("Failed to save: {e}"),
                                std::time::Instant::now(),
                            ));
                        }
                    }
                }

                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            // Status message
            if let Some((msg, time)) = &self.status_msg {
                if time.elapsed().as_secs() < 3 {
                    ui.label(
                        egui::RichText::new(msg)
                            .color(egui::Color32::from_rgb(100, 200, 100)),
                    );
                } else {
                    self.status_msg = None;
                }
            }
        });
    }
}
