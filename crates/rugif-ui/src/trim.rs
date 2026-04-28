use std::ops::Range;

use eframe::egui;
use rugif_core::CaptureFrame;

/// User's choice from the trim dialog.
pub enum TrimResult {
    /// Encode frames in `start..end` (half-open).
    Save(Range<usize>),
    /// Discard the recording — no file written.
    Cancel,
}

/// Show a trim window that lets the user pick a start/end frame.
/// Blocks until the user clicks Save / Cancel or closes the window.
pub fn show_trim_dialog(
    frames: &[CaptureFrame],
    fps: u8,
) -> Result<TrimResult, Box<dyn std::error::Error>> {
    if frames.is_empty() {
        return Ok(TrimResult::Cancel);
    }

    let result = std::sync::Arc::new(std::sync::Mutex::new(TrimResult::Cancel));
    let result_for_app = result.clone();

    // egui requires the App to be 'static, so we can't borrow the slice into it.
    // Build lightweight previews up-front (one ColorImage per frame, lazy uploaded
    // as textures) so the App owns its data.
    let previews: Vec<FramePreview> = frames
        .iter()
        .map(|f| FramePreview {
            image: egui::ColorImage::from_rgba_unmultiplied(
                [f.width as usize, f.height as usize],
                &f.data,
            ),
        })
        .collect();

    let options = crate::native_options_any_thread(
        egui::ViewportBuilder::default()
            .with_decorations(true)
            .with_inner_size([820.0, 460.0])
            .with_min_inner_size([520.0, 320.0])
            .with_title("rugif — Trim Recording"),
    );

    eframe::run_native(
        "rugif - Trim",
        options,
        Box::new(move |_cc| Ok(Box::new(TrimApp::new(previews, fps, result_for_app)))),
    )?;

    let lock = result.lock().unwrap();
    Ok(match &*lock {
        TrimResult::Save(r) => TrimResult::Save(r.clone()),
        TrimResult::Cancel => TrimResult::Cancel,
    })
}

struct FramePreview {
    image: egui::ColorImage,
}

struct TrimApp {
    previews: Vec<FramePreview>,
    fps: u8,
    start_idx: usize,
    end_idx: usize,
    /// Cached preview textures for start/end. Rebuilt when the index changes.
    start_tex: Option<egui::TextureHandle>,
    end_tex: Option<egui::TextureHandle>,
    cached_start_idx: Option<usize>,
    cached_end_idx: Option<usize>,
    result: std::sync::Arc<std::sync::Mutex<TrimResult>>,
}

impl TrimApp {
    fn new(
        previews: Vec<FramePreview>,
        fps: u8,
        result: std::sync::Arc<std::sync::Mutex<TrimResult>>,
    ) -> Self {
        let last = previews.len().saturating_sub(1);
        Self {
            start_idx: 0,
            end_idx: last,
            previews,
            fps,
            start_tex: None,
            end_tex: None,
            cached_start_idx: None,
            cached_end_idx: None,
            result,
        }
    }

    fn finish(&self, ctx: &egui::Context, result: TrimResult) {
        *self.result.lock().unwrap() = result;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn duration_label(&self) -> String {
        let total_frames = self.previews.len();
        let selected = self.end_idx - self.start_idx + 1;
        let total_secs = total_frames as f64 / self.fps as f64;
        let selected_secs = selected as f64 / self.fps as f64;
        format!(
            "Selected {selected_secs:.2}s ({selected} frames)  /  Total {total_secs:.2}s ({total_frames} frames)"
        )
    }
}

impl eframe::App for TrimApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Refresh preview textures only when the indices change.
        if self.cached_start_idx != Some(self.start_idx) {
            let img = self.previews[self.start_idx].image.clone();
            self.start_tex = Some(ctx.load_texture("trim_start", img, egui::TextureOptions::LINEAR));
            self.cached_start_idx = Some(self.start_idx);
        }
        if self.cached_end_idx != Some(self.end_idx) {
            let img = self.previews[self.end_idx].image.clone();
            self.end_tex = Some(ctx.load_texture("trim_end", img, egui::TextureOptions::LINEAR));
            self.cached_end_idx = Some(self.end_idx);
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.finish(ctx, TrimResult::Cancel);
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Trim Recording");
            ui.label(self.duration_label());
            ui.separator();

            let last_idx = self.previews.len().saturating_sub(1);
            let preview_max = egui::vec2(360.0, 240.0);

            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.label(format!(
                        "Start — frame {} ({:.2}s)",
                        self.start_idx,
                        self.start_idx as f64 / self.fps as f64,
                    ));
                    if let Some(tex) = &self.start_tex {
                        let size = fit_size(tex.size_vec2(), preview_max);
                        ui.image((tex.id(), size));
                    }
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(format!(
                        "End — frame {} ({:.2}s)",
                        self.end_idx,
                        self.end_idx as f64 / self.fps as f64,
                    ));
                    if let Some(tex) = &self.end_tex {
                        let size = fit_size(tex.size_vec2(), preview_max);
                        ui.image((tex.id(), size));
                    }
                });
            });

            ui.add_space(8.0);
            ui.separator();

            // Sliders. Start <= End is enforced by the slider ranges.
            ui.horizontal(|ui| {
                ui.label("Start:");
                ui.add(
                    egui::Slider::new(&mut self.start_idx, 0..=self.end_idx)
                        .show_value(true)
                        .text("frame"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("End:  ");
                ui.add(
                    egui::Slider::new(&mut self.end_idx, self.start_idx..=last_idx)
                        .show_value(true)
                        .text("frame"),
                );
            });

            ui.add_space(12.0);
            ui.separator();

            ui.horizontal(|ui| {
                let save_btn = egui::Button::new(
                    egui::RichText::new("Save trimmed")
                        .color(egui::Color32::WHITE)
                        .size(14.0),
                )
                .fill(egui::Color32::from_rgb(40, 130, 200));
                if ui.add(save_btn).clicked() {
                    let range = self.start_idx..(self.end_idx + 1);
                    self.finish(ctx, TrimResult::Save(range));
                    return;
                }

                if ui.button("Save full").clicked() {
                    let range = 0..self.previews.len();
                    self.finish(ctx, TrimResult::Save(range));
                    return;
                }

                let cancel_btn = egui::Button::new(
                    egui::RichText::new("Cancel").color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(150, 60, 60));
                if ui.add(cancel_btn).clicked() {
                    self.finish(ctx, TrimResult::Cancel);
                }
            });
        });
    }
}

/// Scale `src` to fit inside `bounds` while preserving aspect ratio.
fn fit_size(src: egui::Vec2, bounds: egui::Vec2) -> egui::Vec2 {
    if src.x <= 0.0 || src.y <= 0.0 {
        return bounds;
    }
    let scale = (bounds.x / src.x).min(bounds.y / src.y).min(1.0);
    egui::vec2(src.x * scale, src.y * scale)
}
