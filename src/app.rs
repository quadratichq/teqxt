use std::sync::Arc;

use egui::emath::GuiRounding;
use egui::mutex::RwLock;
use egui::{TextureId, emath};

use crate::gfx::{DrawParams, Gfx};

pub struct App {
    gfx: Arc<Gfx>,
    renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    texture_id: TextureId,

    /// Font size, measured in pixels per em.
    px_per_em: f32,
    /// Pixel scale.
    ///
    /// This emulates a display with lower pixel density.
    pixel_scale: u32,
    /// Translation of the viewport.
    translation: egui::Vec2,

    /// Points, stored in em coordinates.
    points: Vec<egui::Pos2>,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let wgpu_render_state = cc
            .wgpu_render_state
            .clone()
            .expect("missing wgpu_render_state");
        let egui_wgpu::RenderState {
            adapter,
            device,
            queue,
            target_format,
            renderer,
            ..
        } = wgpu_render_state;

        let gfx = Arc::new(Gfx::new(adapter, device, queue, target_format));

        let texture_id = renderer.write().register_native_texture(
            &gfx.device,
            &gfx.output_texture
                .lock()
                .create_view(&wgpu::TextureViewDescriptor::default()),
            wgpu::FilterMode::Nearest,
        );

        cc.egui_ctx.style_mut(|style| {
            style.spacing.slider_width *= 3.0;
        });

        Self {
            gfx,
            renderer,
            texture_id,

            px_per_em: 32.0,
            pixel_scale: 1,
            translation: egui::Vec2::ZERO,

            points: vec![
                egui::pos2(0.0, 0.0),
                egui::pos2(1.0, 6.0),
                egui::pos2(4.0, 3.0),
            ],
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::new(egui::panel::Side::Left, "left_panel").show(ctx, |ui| {
            ui.scope(|ui| {
                ui.label("Font size");
                ui.add(egui::Slider::new(&mut self.px_per_em, 1.0..=100.0).logarithmic(true));
            })
            .response
            .on_hover_text("Measured in pixels per em. Double this to emulate HiDPI.");

            ui.separator();

            ui.scope(|ui| {
                ui.label("Pixel scale");
                ui.add(egui::Slider::new(&mut self.pixel_scale, 1..=100).logarithmic(true));
            })
            .response
            .on_hover_text("This emulates a display with lower pixel density.");

            ui.separator();

            if ui.button("Reset translation").clicked() {
                self.translation = egui::Vec2::ZERO;
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let egui_rect = ui
                .available_rect_before_wrap()
                .round_to_pixels(ui.pixels_per_point() * self.pixel_scale as f32);
            let px_rect_size =
                (egui_rect.size() * ui.pixels_per_point() / self.pixel_scale as f32).round();
            let em_rect_size = px_rect_size / self.px_per_em;
            let em_rect = egui::Rect::from_center_size(egui::Pos2::ZERO, em_rect_size);
            let egui_to_em = emath::RectTransform::from_to(egui_rect, em_rect);
            let em_to_egui = egui_to_em.inverse();

            // Update output size
            self.gfx
                .set_output_size(px_rect_size.x as u32, px_rect_size.y as u32);

            // NDC = normalized device coordinates (-1 to +1 for the whole texture)
            let em_per_ndc = px_rect_size / 2.0 / self.px_per_em;
            crate::gfx::draw(
                &self.gfx,
                DrawParams {
                    scale: [1.0 / em_per_ndc.x, 1.0 / em_per_ndc.y],
                    points: [self.points[0], self.points[1], self.points[2]]
                        .map(|pos| pos + self.translation)
                        .map(|pos| [pos.x, pos.y]),
                },
            );

            // Update egui texture
            self.renderer.write().update_egui_texture_from_wgpu_texture(
                &self.gfx.device,
                &self
                    .gfx
                    .output_texture
                    .lock()
                    .create_view(&wgpu::TextureViewDescriptor::default()),
                wgpu::FilterMode::Nearest,
                self.texture_id,
            );

            let r = egui::Frame::canvas(ui.style()).show(ui, |ui| {
                // Draw egui texture
                ui.put(
                    egui_rect,
                    egui::Image::new((self.texture_id, egui_rect.size())),
                );

                // Draw points and handle drag interaction
                for (i, em_pos) in self.points.iter_mut().enumerate() {
                    let egui_pos = em_to_egui.transform_pos(*em_pos + self.translation);
                    let radius = 10.0;
                    let r = ui.interact(
                        egui::Rect::from_center_size(egui_pos, egui::Vec2::splat(radius)),
                        ui.auto_id_with(("control_point", i)),
                        egui::Sense::drag(),
                    );
                    let stroke = ui.style().interact(&r).fg_stroke;
                    ui.painter().circle_stroke(egui_pos, 10.0, stroke);
                    let egui_delta = r.drag_delta();
                    let em_delta = egui_delta * egui_to_em.scale();
                    *em_pos += em_delta;
                }
            });

            let r = ui.interact(
                r.response.rect,
                ui.auto_id_with("frame"),
                egui::Sense::drag(),
            );

            // Handle canvas drag interaction
            let egui_delta = r.drag_delta();
            let em_delta = egui_delta * egui_to_em.scale();
            self.translation += em_delta;
        });
    }
}
