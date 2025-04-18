use std::sync::Arc;

use egui::TextureId;
use egui::emath::GuiRounding;
use egui::mutex::RwLock;

use crate::gfx::Gfx;

pub struct App {
    gfx: Arc<Gfx>,
    renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    texture_id: TextureId,
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

        Self {
            gfx,
            renderer,
            texture_id,
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui
                .available_rect_before_wrap()
                .round_to_pixels(ui.pixels_per_point());

            // Update output size
            self.gfx
                .set_output_size(rect.width() as u32, rect.height() as u32);

            crate::gfx::draw(&self.gfx);

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

            ui.put(rect, egui::Image::new((self.texture_id, rect.size())));
        });
    }
}
