use std::sync::Arc;

use egui::emath::GuiRounding;
use egui::mutex::RwLock;
use egui::{TextureId, emath};
use itertools::Itertools;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontWeight, Layout, LayoutContext, StyleProperty,
};
use swash::FontRef;
use swash::zeno::{PathData, Vector};

use crate::gfx::{DrawParams, Gfx, Glyph};

/// "Hello" written using several different scripts
const GREETINGS: &[&str] = &[
    "Hello!",                   // Latin (English)
    "السلام عليكم",              // Arabic
    "سَلام",                      // Persian (Farsi)
    "नमस्ते",                     // Devanagari (Hindi)
    "こんにちは",               // Katakana (Japanese)
    "안녕하세요",               // Hangul (Korean)
    "您好",                     // Chinese (Mandarin)
    "здравствуйтеzdravstvuyte", // Cyrillic (Russian)
];

pub struct App {
    gfx: Arc<Gfx>,
    renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    texture_id: TextureId,

    font_ref: FontRef<'static>,
    font_ctx: FontContext,

    /// Font size, measured in pixels per em.
    px_per_em: f32,
    /// Pixel scale.
    ///
    /// This emulates a display with lower pixel density.
    pixel_scale: u32,
    /// Translation of the viewport.
    translation: egui::Vec2,

    /// Text to render.
    text: String,
    glyphs: Vec<Glyph>,

    initial: bool,
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

        let font_data = std::fs::read("/Library/Fonts/Arial Unicode.ttf")
            .expect("error reading font from /Library/Fonts/Arial Unicode.ttf");
        let font_ref =
            FontRef::from_index(font_data.clone().leak(), 0).expect("error loading font");
        let mut font_ctx = FontContext::new();
        font_ctx.collection.register_fonts(font_data);

        Self {
            gfx,
            renderer,
            texture_id,

            font_ref,
            font_ctx,

            px_per_em: 72.0,
            pixel_scale: 1,
            translation: egui::Vec2::ZERO,

            text: GREETINGS.iter().join("\n"),
            glyphs: vec![],

            initial: true,
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::new(egui::panel::Side::Left, "left_panel").show(ctx, |ui| {
            let mut font_size_changed = false;
            ui.scope(|ui| {
                ui.label("Font size");
                let r =
                    ui.add(egui::Slider::new(&mut self.px_per_em, 1.0..=7200.0).logarithmic(true));
                font_size_changed |= r.changed();
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

            ui.separator();

            // let context = swash::scale::ScaleContext::new();
            // context.builder(self.font);

            if ui.text_edit_multiline(&mut self.text).changed() || std::mem::take(&mut self.initial)
            {
                let mut layout_ctx = LayoutContext::new();

                let mut builder = layout_ctx.ranged_builder(&mut self.font_ctx, &self.text, 1.0);
                builder.push_default(StyleProperty::FontStack(parley::FontStack::Single(
                    parley::FontFamily::Named("Arial Unicode MS".into()),
                )));
                builder.push_default(StyleProperty::LineHeight(1.3));
                builder.push_default(StyleProperty::FontSize(1.0));
                builder.push(StyleProperty::FontWeight(FontWeight::new(600.0)), ..);
                let mut layout: Layout<()> = builder.build(&self.text);
                layout.break_all_lines(None);
                layout.align(None, Alignment::Start, AlignmentOptions::default());

                let mut scale_ctx = swash::scale::ScaleContext::new();
                let mut scaler = scale_ctx.builder(self.font_ref).size(1.0).build();

                let mut output = vec![];

                for line in layout.lines() {
                    for item in line.items() {
                        match item {
                            parley::PositionedLayoutItem::GlyphRun(glyph_run) => {
                                for glyph in glyph_run.positioned_glyphs() {
                                    if let Some(outline) = scaler.scale_outline(glyph.id) {
                                        let mut curves = vec![];
                                        let mut last_point = Vector::ZERO;
                                        let mut start_of_subpath = Vector::ZERO;
                                        for command in outline.path().commands() {
                                            match command {
                                                swash::zeno::Command::MoveTo(vector) => {
                                                    start_of_subpath = vector;
                                                    last_point = vector;
                                                }
                                                swash::zeno::Command::LineTo(vector) => {
                                                    curves.push([
                                                        last_point,
                                                        (last_point + vector) * 0.5,
                                                        vector,
                                                    ]);
                                                    last_point = vector;
                                                }
                                                swash::zeno::Command::CurveTo(..) => {
                                                    todo!("cubic bezier is not implemented")
                                                }
                                                swash::zeno::Command::QuadTo(vector, vector1) => {
                                                    curves.push([last_point, vector, vector1]);
                                                    last_point = vector1;
                                                }
                                                swash::zeno::Command::Close => {
                                                    curves.push([
                                                        last_point,
                                                        (last_point + start_of_subpath) * 0.5,
                                                        start_of_subpath,
                                                    ]);
                                                }
                                            }
                                        }
                                        output.push(Glyph {
                                            offset: [glyph.x, -glyph.y],
                                            curves: curves
                                                .into_iter()
                                                .map(|curve| {
                                                    curve.map(|v| [v.x + glyph.x, v.y - glyph.y])
                                                })
                                                .collect(),
                                        });
                                    }
                                }
                            }
                            parley::PositionedLayoutItem::InlineBox(_positioned_inline_box) => {
                                todo!("handle inline box")
                            }
                        }
                    }
                }

                self.glyphs = output;
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

            // Update output size
            self.gfx
                .set_output_size(px_rect_size.x as u32, px_rect_size.y as u32);

            // NDC = normalized device coordinates (-1 to +1 for the whole texture)
            let em_per_ndc = px_rect_size / 2.0 / self.px_per_em;
            crate::gfx::draw(
                &self.gfx,
                DrawParams {
                    scale: [1.0 / em_per_ndc.x, 1.0 / em_per_ndc.y],
                    translation: self.translation.into(),
                    glyphs: self.glyphs.clone(),
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
            });

            let r = ui.interact(
                r.response.rect,
                ui.auto_id_with("frame"),
                egui::Sense::drag(),
            );

            // Handle canvas drag interaction
            let egui_delta = r.drag_delta();
            let em_delta = egui_delta * egui_to_em.scale() * egui::vec2(1.0, -1.0);
            self.translation += em_delta;
        });
    }
}
