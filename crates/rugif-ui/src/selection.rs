use eframe::egui;
use rugif_core::Region;

/// Result of the selection overlay.
pub enum SelectionResult {
    /// User selected a region.
    Selected(Region),
    /// User cancelled (Escape).
    Cancelled,
}

/// Run a fullscreen overlay that displays a frozen screenshot and lets the user
/// drag-select a rectangular region. Returns the selected region or None if cancelled.
pub fn select_region(
    screenshot_data: Vec<u8>,
    screenshot_width: u32,
    screenshot_height: u32,
) -> Result<SelectionResult, Box<dyn std::error::Error>> {
    let result = std::sync::Arc::new(std::sync::Mutex::new(None::<SelectionResult>));
    let result_clone = result.clone();

    let options = crate::native_options_any_thread(
        egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_fullscreen(true),
    );

    eframe::run_native(
        "rugif - Select Region",
        options,
        Box::new(move |cc| {
            let app = SelectionApp::new(
                cc,
                screenshot_data,
                screenshot_width,
                screenshot_height,
                result_clone,
            );
            Ok(Box::new(app))
        }),
    )?;

    let lock = result.lock().unwrap();
    Ok(lock.clone().unwrap_or(SelectionResult::Cancelled))
}

impl Clone for SelectionResult {
    fn clone(&self) -> Self {
        match self {
            Self::Selected(r) => Self::Selected(*r),
            Self::Cancelled => Self::Cancelled,
        }
    }
}

struct SelectionApp {
    /// The screenshot texture displayed as background.
    texture: egui::TextureHandle,
    /// Drag state: start position in screen pixels.
    drag_start: Option<egui::Pos2>,
    /// Current mouse position during drag.
    drag_current: Option<egui::Pos2>,
    /// Final result communicated back to the caller.
    result: std::sync::Arc<std::sync::Mutex<Option<SelectionResult>>>,
}

impl SelectionApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        screenshot_data: Vec<u8>,
        width: u32,
        height: u32,
        result: std::sync::Arc<std::sync::Mutex<Option<SelectionResult>>>,
    ) -> Self {
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            &screenshot_data,
        );

        let texture = cc.egui_ctx.load_texture(
            "screenshot",
            image,
            egui::TextureOptions::LINEAR,
        );

        Self {
            texture,
            drag_start: None,
            drag_current: None,
            result,
        }
    }

    fn selection_rect(&self) -> Option<egui::Rect> {
        if let (Some(start), Some(current)) = (self.drag_start, self.drag_current) {
            let min = egui::pos2(start.x.min(current.x), start.y.min(current.y));
            let max = egui::pos2(start.x.max(current.x), start.y.max(current.y));
            let rect = egui::Rect::from_min_max(min, max);
            if rect.width() > 5.0 && rect.height() > 5.0 {
                return Some(rect);
            }
        }
        None
    }

    fn finish(&self, ctx: &egui::Context, result: SelectionResult) {
        *self.result.lock().unwrap() = Some(result);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for SelectionApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.finish(ctx, SelectionResult::Cancelled);
            return;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let screen_rect = ui.max_rect();

                // Draw the screenshot filling the screen.
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(
                    self.texture.id(),
                    screen_rect,
                    uv,
                    egui::Color32::WHITE,
                );

                // Dark overlay on top of the screenshot.
                let dim_color = egui::Color32::from_black_alpha(140);

                if let Some(sel_rect) = self.selection_rect() {
                    // Draw dim overlay everywhere except the selection
                    // Top
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(screen_rect.min, egui::pos2(screen_rect.max.x, sel_rect.min.y)),
                        0.0,
                        dim_color,
                    );
                    // Bottom
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(egui::pos2(screen_rect.min.x, sel_rect.max.y), screen_rect.max),
                        0.0,
                        dim_color,
                    );
                    // Left
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(screen_rect.min.x, sel_rect.min.y),
                            egui::pos2(sel_rect.min.x, sel_rect.max.y),
                        ),
                        0.0,
                        dim_color,
                    );
                    // Right
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(sel_rect.max.x, sel_rect.min.y),
                            egui::pos2(screen_rect.max.x, sel_rect.max.y),
                        ),
                        0.0,
                        dim_color,
                    );

                    // Selection border
                    ui.painter().rect_stroke(
                        sel_rect,
                        0.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 150, 255)),
                        egui::StrokeKind::Outside,
                    );

                    // Dimension label
                    let w = sel_rect.width() as u32;
                    let h = sel_rect.height() as u32;
                    let label = format!("{w} x {h}");
                    let label_pos = egui::pos2(
                        sel_rect.min.x,
                        sel_rect.min.y - 20.0,
                    );
                    ui.painter().text(
                        label_pos,
                        egui::Align2::LEFT_BOTTOM,
                        label,
                        egui::FontId::proportional(14.0),
                        egui::Color32::WHITE,
                    );
                } else {
                    // No selection yet — dim the whole screen
                    ui.painter().rect_filled(screen_rect, 0.0, dim_color);

                    // Instruction text
                    ui.painter().text(
                        screen_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Click and drag to select a region\nPress Escape to cancel",
                        egui::FontId::proportional(24.0),
                        egui::Color32::WHITE,
                    );
                }

                // Handle mouse input
                let response = ui.allocate_rect(screen_rect, egui::Sense::click_and_drag());

                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.drag_start = Some(pos);
                        self.drag_current = Some(pos);
                    }
                }

                if response.dragged_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.drag_current = Some(pos);
                    }
                }

                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    if let Some(sel_rect) = self.selection_rect() {
                        let region = Region {
                            x: sel_rect.min.x as i32,
                            y: sel_rect.min.y as i32,
                            width: sel_rect.width() as u32,
                            height: sel_rect.height() as u32,
                        };
                        self.finish(ctx, SelectionResult::Selected(region));
                    } else {
                        // Too small, reset
                        self.drag_start = None;
                        self.drag_current = None;
                    }
                }
            });

        // Continuous repaint while dragging for smooth feedback
        ctx.request_repaint();
    }
}
