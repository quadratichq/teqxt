use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::emath::GuiRounding;
use egui::mutex::RwLock;
use egui::{TextureId, emath};
use itertools::Itertools;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontWeight, Layout, LayoutContext, StyleProperty,
};
use swash::FontRef;
use swash::zeno::{PathData, Vector};

use crate::gfx::{
    DrawParams, Gfx, Glyph, GlyphCache, GlyphCacheKey, RenderStats, Renderer, cubic_to_quadratics,
};

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

/// War and Peace by Leo Tolstoy - embedded at compile time for stress testing.
const WAR_AND_PEACE: &str = include_str!("../data/book-war-and-peace.txt");

/// Tolerance for cubic-to-quadratic bezier conversion (in font units).
const CUBIC_TOLERANCE: f32 = 0.01;

/// Get a portion of War and Peace by percentage (0-100).
fn get_book_portion(percent: usize) -> &'static str {
    let percent = percent.min(100);
    let char_count = (WAR_AND_PEACE.len() * percent) / 100;

    // Find a good break point (end of line) near the target
    if char_count >= WAR_AND_PEACE.len() {
        WAR_AND_PEACE
    } else {
        // Find the next newline after the target to avoid cutting mid-word
        let end = WAR_AND_PEACE[char_count..]
            .find('\n')
            .map(|i| char_count + i)
            .unwrap_or(char_count);
        &WAR_AND_PEACE[..end]
    }
}

/// Performance statistics.
#[derive(Default)]
struct PerfStats {
    last_frame_time: Duration,
    last_glyph_gen_time: Duration,
    frame_times: Vec<Duration>,
    avg_frame_time: Duration,
}

impl PerfStats {
    fn update_frame_time(&mut self, duration: Duration) {
        self.last_frame_time = duration;
        self.frame_times.push(duration);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }
        let total: Duration = self.frame_times.iter().sum();
        self.avg_frame_time = total / self.frame_times.len() as u32;
    }

    fn fps(&self) -> f32 {
        if self.avg_frame_time.as_secs_f32() > 0.0 {
            1.0 / self.avg_frame_time.as_secs_f32()
        } else {
            0.0
        }
    }
}

pub struct App {
    gfx: Gfx,
    egui_renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    text_renderer: Renderer,
    texture_id: TextureId,

    font_ref: FontRef<'static>,
    font_ctx: FontContext,

    /// Cache for glyph curve data to avoid re-extraction.
    glyph_cache: GlyphCache,

    /// Font size, measured in pixels per em.
    px_per_em: f32,
    /// Pixel scale.
    pixel_scale: u32,
    /// Translation of the viewport, in ems.
    translation: egui::Vec2,

    /// The full text to render.
    text: String,
    /// Line width for text wrapping (in ems). None = no wrapping.
    line_width_ems: Option<f32>,
    /// Lines per page for book layout.
    lines_per_page: usize,
    /// Gap between pages (in ems).
    page_gap_ems: f32,

    /// Cached glyphs for rendering.
    glyphs: Arc<Vec<Glyph>>,
    /// Whether glyphs need to be regenerated.
    glyphs_dirty: bool,
    /// Total curve count for stats.
    total_curves: usize,
    /// Number of pages in the current layout.
    page_count: usize,
    /// Grid dimensions (columns, rows).
    grid_dims: (usize, usize),

    gamma: f32,
    prescale: bool,
    hint: bool,
    subpixel_aa: bool,

    initial: bool,

    /// Performance statistics.
    perf_stats: PerfStats,

    /// Render statistics from the GPU renderer.
    render_stats: RenderStats,

    /// Percentage of War and Peace to load (1-100).
    book_percent: usize,
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
            renderer: egui_renderer,
            ..
        } = wgpu_render_state;

        let gfx = Gfx::new(adapter, device, queue, target_format);
        let text_renderer = Renderer::new(&gfx);

        let texture_id = egui_renderer.write().register_native_texture(
            &gfx.device,
            &gfx.create_dummy_texture_view(),
            wgpu::FilterMode::Nearest,
        );

        cc.egui_ctx.style_mut(|style| {
            style.spacing.slider_width *= 3.0;
        });

        // Try multiple font paths for cross-platform support.
        let font_paths = [
            "/Library/Fonts/Arial Unicode.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "C:\\Windows\\Fonts\\ARIALUNI.TTF",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];

        let font_data = font_paths
            .iter()
            .find_map(|path| std::fs::read(path).ok())
            .expect("Could not find a suitable font file");

        let font_ref =
            FontRef::from_index(font_data.clone().leak(), 0).expect("error loading font");
        let mut font_ctx = FontContext::new();
        font_ctx.collection.register_fonts(font_data);

        // Start with a small greeting text
        let initial_text = GREETINGS.iter().join("\n");

        Self {
            gfx,
            egui_renderer,
            text_renderer,
            texture_id,

            font_ref,
            font_ctx,
            glyph_cache: GlyphCache::new(),

            px_per_em: 14.0,
            pixel_scale: 1,
            translation: egui::Vec2::ZERO,

            text: initial_text,
            line_width_ems: Some(35.0), // ~65 chars/line for book-like reading
            lines_per_page: 50,
            page_gap_ems: 2.0,

            glyphs: Arc::new(vec![]),
            glyphs_dirty: true,
            total_curves: 0,
            page_count: 0,
            grid_dims: (1, 1),

            gamma: 2.2,
            prescale: false,
            hint: false,
            subpixel_aa: false,

            initial: true,

            perf_stats: PerfStats::default(),
            render_stats: RenderStats::default(),

            book_percent: 10,
        }
    }

    /// Load a portion of War and Peace.
    fn load_book(&mut self, percent: usize) {
        self.text = get_book_portion(percent).to_string();
        self.book_percent = percent;
        self.glyphs_dirty = true;
    }

    /// Extract curves from a glyph outline path, converting cubics to quadratics.
    fn extract_curves_from_path(
        commands: impl Iterator<Item = swash::zeno::Command>,
    ) -> Vec<[[f32; 2]; 3]> {
        let mut curves = vec![];
        let mut last_point = Vector::ZERO;
        let mut start_of_subpath = Vector::ZERO;

        for command in commands {
            match command {
                swash::zeno::Command::MoveTo(vector) => {
                    start_of_subpath = vector;
                    last_point = vector;
                }
                swash::zeno::Command::LineTo(vector) => {
                    curves.push([
                        [last_point.x, last_point.y],
                        [
                            (last_point.x + vector.x) * 0.5,
                            (last_point.y + vector.y) * 0.5,
                        ],
                        [vector.x, vector.y],
                    ]);
                    last_point = vector;
                }
                swash::zeno::Command::CurveTo(c1, c2, end) => {
                    let quadratics = cubic_to_quadratics(
                        [last_point.x, last_point.y],
                        [c1.x, c1.y],
                        [c2.x, c2.y],
                        [end.x, end.y],
                        CUBIC_TOLERANCE,
                    );
                    curves.extend(quadratics);
                    last_point = end;
                }
                swash::zeno::Command::QuadTo(control, end) => {
                    curves.push([
                        [last_point.x, last_point.y],
                        [control.x, control.y],
                        [end.x, end.y],
                    ]);
                    last_point = end;
                }
                swash::zeno::Command::Close => {
                    if last_point != start_of_subpath {
                        curves.push([
                            [last_point.x, last_point.y],
                            [
                                (last_point.x + start_of_subpath.x) * 0.5,
                                (last_point.y + start_of_subpath.y) * 0.5,
                            ],
                            [start_of_subpath.x, start_of_subpath.y],
                        ]);
                    }
                }
            }
        }

        curves
    }

    /// Regenerate glyphs from the current text with page grid layout.
    fn regenerate_glyphs(&mut self) {
        let start = Instant::now();

        let font_size = if self.prescale { self.px_per_em } else { 1.0 };
        let post_scale = 1.0 / font_size;
        let line_height = 1.4;

        // Split text into lines
        let lines: Vec<&str> = self.text.lines().collect();
        let total_lines = lines.len();

        // Calculate pages
        let lines_per_page = self.lines_per_page.max(1);
        let page_count = (total_lines + lines_per_page - 1) / lines_per_page;
        self.page_count = page_count.max(1);

        // Calculate grid dimensions (as close to square as possible)
        let cols = (self.page_count as f32).sqrt().ceil() as usize;
        let rows = (self.page_count + cols - 1) / cols;
        self.grid_dims = (cols, rows);

        // Calculate page dimensions in ems
        let page_width_ems = self.line_width_ems.unwrap_or(35.0);
        let page_height_ems = lines_per_page as f32 * line_height;
        let gap = self.page_gap_ems;

        let mut scale_ctx = swash::scale::ScaleContext::new();
        let mut scaler = scale_ctx
            .builder(self.font_ref)
            .size(font_size)
            .hint(self.hint)
            .size(1.0)
            .build();

        let mut output = vec![];
        let mut curve_count = 0;

        // Process each page
        for page_idx in 0..self.page_count {
            let start_line = page_idx * lines_per_page;
            let end_line = (start_line + lines_per_page).min(total_lines);

            // Get the text for this page
            let page_text: String = lines[start_line..end_line].join("\n");
            if page_text.is_empty() {
                continue;
            }

            // Calculate page position in grid
            let col = page_idx % cols;
            let row = page_idx / cols;
            let page_offset_x = col as f32 * (page_width_ems + gap);
            let page_offset_y = -(row as f32 * (page_height_ems + gap)); // Negative Y = down

            // Layout this page's text
            let mut layout_ctx = LayoutContext::new();
            let mut builder = layout_ctx.ranged_builder(&mut self.font_ctx, &page_text, 1.0);
            builder.push_default(StyleProperty::FontStack(parley::FontStack::List(
                vec![
                    parley::FontFamily::Named("Open Sans".into()),
                    parley::FontFamily::Named("Arial Unicode MS".into()),
                ]
                .into(),
            )));
            builder.push_default(StyleProperty::LineHeight(line_height));
            builder.push_default(StyleProperty::FontSize(font_size));
            builder.push(StyleProperty::FontWeight(FontWeight::new(400.0)), ..);
            let mut layout: Layout<()> = builder.build(&page_text);

            // Apply line breaking with the configured width
            let max_width = Some(page_width_ems * font_size);
            layout.break_all_lines(max_width);
            layout.align(max_width, Alignment::Start, AlignmentOptions::default());

            let mut init_baseline = None;

            for line in layout.lines() {
                for item in line.items() {
                    match item {
                        parley::PositionedLayoutItem::GlyphRun(glyph_run) => {
                            let baseline = *init_baseline.get_or_insert(glyph_run.baseline());

                            for glyph in glyph_run.positioned_glyphs() {
                                let cache_key = GlyphCacheKey {
                                    glyph_id: u32::from(glyph.id) as u16,
                                    hinted: self.hint,
                                };

                                let cached_shape =
                                    self.glyph_cache.get_or_insert(cache_key, || {
                                        if let Some(outline) = scaler.scale_outline(glyph.id) {
                                            Self::extract_curves_from_path(
                                                outline.path().commands(),
                                            )
                                        } else {
                                            vec![]
                                        }
                                    });

                                if !cached_shape.curves.is_empty() {
                                    curve_count += cached_shape.curves.len();
                                    output.push(Glyph {
                                        offset: [
                                            page_offset_x + glyph.x * post_scale,
                                            page_offset_y + (baseline - glyph.y) * post_scale,
                                        ],
                                        curves: Arc::clone(&cached_shape.curves),
                                    });
                                }
                            }
                        }
                        parley::PositionedLayoutItem::InlineBox(_) => {}
                    }
                }
            }
        }

        self.glyphs = Arc::new(output);
        self.total_curves = curve_count;
        self.glyphs_dirty = false;
        self.perf_stats.last_glyph_gen_time = start.elapsed();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = Instant::now();

        egui::SidePanel::new(egui::panel::Side::Left, "left_panel")
            .min_width(280.0)
            .show(ctx, |ui| {
                ui.heading("📚 War and Peace");
                ui.label("by Leo Tolstoy");
                ui.separator();

                // Book loading controls
                ui.horizontal(|ui| {
                    ui.label("Load:");
                    if ui
                        .add(
                            egui::Slider::new(&mut self.book_percent, 1..=100)
                                .suffix("%")
                                .logarithmic(true),
                        )
                        .changed()
                    {
                        self.load_book(self.book_percent);
                    }
                });

                let book_chars = get_book_portion(self.book_percent).len();
                ui.label(format!(
                    "~{} characters ({:.1} MB)",
                    book_chars,
                    book_chars as f64 / 1_000_000.0
                ));

                ui.horizontal(|ui| {
                    if ui.button("1%").clicked() {
                        self.load_book(1);
                    }
                    if ui.button("5%").clicked() {
                        self.load_book(5);
                    }
                    if ui.button("10%").clicked() {
                        self.load_book(10);
                    }
                    if ui.button("25%").clicked() {
                        self.load_book(25);
                    }
                    if ui.button("50%").clicked() {
                        self.load_book(50);
                    }
                    if ui.button("100%").clicked() {
                        self.load_book(100);
                    }
                });

                ui.separator();

                // Text wrapping settings
                ui.heading("📏 Text Layout");

                let mut wrap_enabled = self.line_width_ems.is_some();
                if ui.checkbox(&mut wrap_enabled, "Word wrap").changed() {
                    if wrap_enabled {
                        self.line_width_ems = Some(35.0);
                    } else {
                        self.line_width_ems = None;
                    }
                    self.glyphs_dirty = true;
                }

                if let Some(ref mut width) = self.line_width_ems {
                    ui.horizontal(|ui| {
                        ui.label("Line width:");
                        if ui
                            .add(
                                egui::Slider::new(width, 20.0..=80.0)
                                    .fixed_decimals(0)
                                    .suffix(" em"),
                            )
                            .changed()
                        {
                            self.glyphs_dirty = true;
                        }
                    });
                    // Show approximate characters per line
                    let approx_chars = (*width * 1.8) as usize; // ~0.55 em per char average
                    ui.label(format!("(~{} chars/line)", approx_chars));
                }

                ui.horizontal(|ui| {
                    ui.label("Lines/page:");
                    if ui
                        .add(
                            egui::Slider::new(&mut self.lines_per_page, 10..=100)
                                .logarithmic(false),
                        )
                        .changed()
                    {
                        self.glyphs_dirty = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Page gap:");
                    if ui
                        .add(
                            egui::Slider::new(&mut self.page_gap_ems, 0.5..=10.0)
                                .fixed_decimals(1)
                                .suffix(" em"),
                        )
                        .changed()
                    {
                        self.glyphs_dirty = true;
                    }
                });

                ui.label(format!(
                    "Grid: {} × {} = {} pages",
                    self.grid_dims.0, self.grid_dims.1, self.page_count
                ));

                ui.separator();

                // Render settings
                ui.heading("⚙ Render Settings");

                ui.checkbox(&mut self.prescale, "Pre-scale");
                if !self.prescale {
                    self.hint = false;
                }
                ui.add_enabled_ui(self.prescale, |ui| {
                    if ui.checkbox(&mut self.hint, "Hint").changed() {
                        self.glyphs_dirty = true;
                    }
                });
                ui.checkbox(&mut self.subpixel_aa, "Subpixel AA");

                ui.horizontal(|ui| {
                    ui.label("Gamma:");
                    ui.add(egui::Slider::new(&mut self.gamma, 0.5..=3.0).fixed_decimals(2));
                });

                ui.horizontal(|ui| {
                    ui.label("Font size:");
                    if ui
                        .add(egui::Slider::new(&mut self.px_per_em, 4.0..=200.0).logarithmic(true))
                        .changed()
                    {
                        // Font size change requires re-layout if prescale is enabled
                        if self.prescale {
                            self.glyphs_dirty = true;
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Pixel scale:");
                    ui.add(egui::Slider::new(&mut self.pixel_scale, 1..=10));
                });

                if ui.button("Reset View").clicked() {
                    self.translation = egui::Vec2::ZERO;
                    self.px_per_em = 14.0;
                }

                ui.separator();

                // Performance stats
                ui.heading("📊 Performance");

                let fps = self.perf_stats.fps();
                let fps_color = if fps >= 55.0 {
                    egui::Color32::GREEN
                } else if fps >= 30.0 {
                    egui::Color32::YELLOW
                } else {
                    egui::Color32::RED
                };
                ui.colored_label(fps_color, format!("FPS: {:.1}", fps));
                ui.label(format!(
                    "Frame time: {:.2}ms",
                    self.perf_stats.avg_frame_time.as_secs_f32() * 1000.0
                ));
                ui.label(format!(
                    "Glyph gen: {:.2}ms",
                    self.perf_stats.last_glyph_gen_time.as_secs_f32() * 1000.0
                ));

                ui.separator();

                // Stats
                ui.heading("📈 Statistics");

                ui.label(format!("Characters: {}", self.text.len()));
                ui.label(format!("Total glyphs: {}", self.render_stats.total_glyphs));
                ui.label(format!(
                    "Visible glyphs: {}",
                    self.render_stats.visible_glyphs
                ));
                ui.label(format!(
                    "Unique curves (atlas): {}",
                    self.render_stats.unique_curves
                ));
                ui.label(format!(
                    "Curve instances: {}",
                    self.render_stats.rendered_curves
                ));
                ui.label(format!("Glyph cache: {} entries", self.glyph_cache.len()));

                // Show memory savings from atlas
                if self.render_stats.unique_curves > 0 && self.render_stats.rendered_curves > 0 {
                    let savings = 1.0
                        - (self.render_stats.unique_curves as f32
                            / self.render_stats.rendered_curves as f32);
                    if savings > 0.0 {
                        ui.colored_label(
                            egui::Color32::GREEN,
                            format!("Atlas savings: {:.0}%", savings * 100.0),
                        );
                    }
                }

                if self.render_stats.curve_limited {
                    ui.colored_label(
                        egui::Color32::RED,
                        "⚠ Curve limit reached! Zoom in for detail.",
                    );
                }

                ui.separator();

                // Other tests
                if ui.button("🌍 Multilingual Test").clicked() {
                    self.text = GREETINGS.iter().join("\n");
                    self.book_percent = 0;
                    self.glyphs_dirty = true;
                }

                ui.separator();

                ui.label("Drag to pan, scroll to zoom");

                // Regenerate glyphs if needed.
                if self.glyphs_dirty || std::mem::take(&mut self.initial) {
                    self.regenerate_glyphs();
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

            let (output_texture_view, render_stats) = self.text_renderer.draw(DrawParams {
                output_size: [px_rect_size.x as u32, px_rect_size.y as u32],
                px_per_em: self.px_per_em,
                translation: self.translation.into(),
                glyphs: Arc::clone(&self.glyphs),
                gamma: self.gamma,
                subpixel_aa: self.subpixel_aa,
            });
            self.render_stats = render_stats;

            self.egui_renderer
                .write()
                .update_egui_texture_from_wgpu_texture(
                    &self.gfx.device,
                    &output_texture_view,
                    wgpu::FilterMode::Nearest,
                    self.texture_id,
                );

            let r = egui::Frame::canvas(ui.style()).show(ui, |ui| {
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

            // Handle canvas drag interaction.
            let egui_delta = r.drag_delta();
            let em_delta = egui_delta * egui_to_em.scale() * egui::vec2(1.0, -1.0);
            self.translation += em_delta;

            // Handle canvas scale interaction.
            if let Some(pos) = r.hover_pos() {
                let get_em_vec_from_center_of_canvas = |p: egui::Pos2, px_per_em: f32| {
                    let egui_vec_from_center_of_canvas = p - egui_rect.center();
                    let pixel_vec_from_center_of_canvas =
                        egui_vec_from_center_of_canvas * ui.pixels_per_point();
                    let em_vec_from_center_of_canvas = pixel_vec_from_center_of_canvas / px_per_em;
                    em_vec_from_center_of_canvas * egui::vec2(1.0, -1.0)
                };
                self.translation -= get_em_vec_from_center_of_canvas(pos, self.px_per_em);
                self.px_per_em *= ui.input(|input| input.zoom_delta());
                self.translation += get_em_vec_from_center_of_canvas(pos, self.px_per_em);
            }
        });

        // Update performance stats
        self.perf_stats.update_frame_time(frame_start.elapsed());

        // Request continuous repaint for FPS counter
        ctx.request_repaint();
    }
}
