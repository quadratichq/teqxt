use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use super::{
    Gfx, SAMPLE_TEXTURE_FORMAT,
    bindings::{CURVE_ATLAS_BINDING, SAMPLE_TEXTURE_BINDING, UNIFORM_BINDING},
    cached::*,
    pipelines::Pipelines,
    structs::*,
};

/// Sample locations, based on [a blog post by Evan Wallace][evanwallace].
///
/// [evanwallace]:
///     https://medium.com/@evanwallace/easy-scalable-text-rendering-on-the-gpu-c3f4d782c5ac,
pub const SAMPLES: [([f32; 2], [f32; 4]); 6] = {
    // Store metadata in alpha channel on all samples to ensure that every pixel
    // gets some metadata.
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    [
        ([0.0 / 6.0, 4.0 / 6.0], BLUE),
        ([1.0 / 6.0, 1.0 / 6.0], BLUE),
        ([2.0 / 6.0, 5.0 / 6.0], GREEN),
        ([3.0 / 6.0, 2.0 / 6.0], GREEN),
        ([4.0 / 6.0, 3.0 / 6.0], RED),
        ([5.0 / 6.0, 0.0 / 6.0], RED),
    ]
};

/// Number of samples per pixel.
pub const SAMPLE_COUNT: u32 = SAMPLES.len() as u32;

/// Maximum number of bezier curves to render per frame.
/// This prevents GPU buffer overflow (256MB limit / 32 bytes per curve ≈ 8M curves).
/// We use a conservative limit to leave room for other GPU resources.
pub const MAX_CURVES_PER_FRAME: usize = 2_000_000;

#[derive(Debug, Clone)]
pub struct DrawParams {
    /// Size of the output texture, in pixels.
    pub output_size: [u32; 2],
    /// Number of pixels in the texture per em in the font.
    pub px_per_em: f32,
    /// XY em-space coordinates to be displayed at the center of screen.
    ///
    /// This should be rounded to the nearest pixel in earlier stages of
    /// processing, if desired.
    pub translation: [f32; 2],
    /// Glyphs to render (uses Arc to avoid cloning).
    pub glyphs: Arc<Vec<Glyph>>,
    pub gamma: f32,
    pub subpixel_aa: bool,
}

#[derive(Debug, Clone)]
pub struct Glyph {
    /// XY offset of the glyph, measured in ems.
    pub offset: [f32; 2],
    /// Bezier curve data for the glyph, measured in ems.
    /// Uses Arc to share curve data between identical glyphs.
    pub curves: Arc<Vec<[[f32; 2]; 3]>>,
}

/// Rendering statistics returned from draw().
#[derive(Debug, Clone, Default)]
pub struct RenderStats {
    /// Total glyphs in the scene.
    pub total_glyphs: usize,
    /// Glyphs that passed viewport culling.
    pub visible_glyphs: usize,
    /// Unique curves in the atlas (shared across all glyph instances).
    pub unique_curves: usize,
    /// Curve instances actually rendered (after culling and limits).
    pub rendered_curves: usize,
    /// Whether rendering was limited due to MAX_CURVES_PER_FRAME.
    pub curve_limited: bool,
}

/// GPU state for font rendering using a 2-pass method similar to the one
/// described in [a blog post by Evan Wallace][evanwallace].
///
/// The first pass consists of several draw calls, each accumulating one more
/// sample per pixel.
///
/// The second pass ("output pass") consists of one draw call that counts the
/// samples for each pixel and determines their final color.
///
/// [evanwallace]:
///     https://medium.com/@evanwallace/easy-scalable-text-rendering-on-the-gpu-c3f4d782c5ac,.
pub struct Renderer {
    /// Graphics driver state.
    pub gfx: Gfx,

    /// Texture to accumulate samples during the first pass.
    pub first_pass_texture: Cached<wgpu::Extent3d, wgpu::Texture>,
    /// Texture to store colors during the output pass.
    pub output_pass_texture: Cached<wgpu::Extent3d, wgpu::Texture>,

    /// Uniform buffer for the first pass (contains all 6 sample variations).
    pub first_pass_uniform_buffer: CachedBuffer<FirstPassUniform>,
    /// Uniform buffer for the output pass.
    pub output_pass_uniform_buffer: wgpu::Buffer,

    // === Atlas-based rendering ===
    /// Storage buffer containing unique glyph curves.
    pub curve_atlas_buffer: CachedBuffer<AtlasCurve>,
    /// Maps Arc pointer to (start_index, count) in the atlas.
    pub atlas_index: HashMap<usize, (u32, u32)>,
    /// Instance buffer for curve rendering.
    pub curve_instance_buffer: CachedBuffer<CurveInstance>,
    /// Cached bind group for atlas-based first pass.
    pub atlas_bind_group: Option<wgpu::BindGroup>,

    /// Cached bind group for the output pass (invalidated when texture size changes).
    pub output_pass_bind_group: Option<(wgpu::Extent3d, wgpu::BindGroup)>,

    /// Shader pipelines.
    pub pipelines: Pipelines,

    /// Last first pass uniform data for dirty checking.
    last_first_pass_params: Option<(f32, [f32; 2], [f32; 2])>,
    /// Last output pass uniform data for dirty checking.
    last_output_pass_uniform: Option<OutputPassUniform>,
}

impl Renderer {
    pub fn new(gfx: &Gfx) -> Self {
        let default_texture_descriptor = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d::default(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gfx.target_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };

        // Create output pass uniform buffer (fixed size, reused).
        let output_pass_uniform_buffer = gfx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("output_pass_uniform_buffer"),
            size: OutputPassUniform::WGPU_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            gfx: gfx.clone(),

            first_pass_texture: Cached::new(gfx, move |gfx, size| {
                gfx.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("teqxt_first_pass_texture"),
                    size,
                    format: SAMPLE_TEXTURE_FORMAT,
                    ..default_texture_descriptor
                })
            }),
            output_pass_texture: Cached::new(gfx, move |gfx, size| {
                gfx.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("teqxt_output_pass_texture"),
                    size,
                    ..default_texture_descriptor
                })
            }),

            first_pass_uniform_buffer: CachedBuffer::new(
                gfx,
                "first_pass_uniform_buffer",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            ),
            output_pass_uniform_buffer,

            // Atlas-based rendering
            curve_atlas_buffer: CachedBuffer::new(
                gfx,
                "curve_atlas_buffer",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            ),
            atlas_index: HashMap::new(),
            curve_instance_buffer: CachedBuffer::new(
                gfx,
                "curve_instance_buffer",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            ),
            atlas_bind_group: None,

            output_pass_bind_group: None,

            pipelines: Pipelines::new(gfx),

            last_first_pass_params: None,
            last_output_pass_uniform: None,
        }
    }

    pub fn draw(&mut self, params: DrawParams) -> (wgpu::TextureView, RenderStats) {
        let mut stats = RenderStats {
            total_glyphs: params.glyphs.len(),
            ..Default::default()
        };

        // Avoid crash on resizing texture.
        if params.output_size[0] == 0 || params.output_size[1] == 0 {
            return (self.gfx.create_dummy_texture_view(), stats);
        }

        let device = &self.gfx.device;

        let size = wgpu::Extent3d {
            width: params.output_size[0],
            height: params.output_size[1],
            depth_or_array_layers: 1,
        };

        // Calculate viewport bounds in em space for culling.
        let viewport_half_width = (params.output_size[0] as f32 / params.px_per_em) / 2.0;
        let viewport_half_height = (params.output_size[1] as f32 / params.px_per_em) / 2.0;
        let viewport_center_x = -params.translation[0];
        let viewport_center_y = -params.translation[1];
        let margin = 2.0; // ems
        let viewport_min_x = viewport_center_x - viewport_half_width - margin;
        let viewport_max_x = viewport_center_x + viewport_half_width + margin;
        let viewport_min_y = viewport_center_y - viewport_half_height - margin;
        let viewport_max_y = viewport_center_y + viewport_half_height + margin;

        // Get/create textures (cached by size).
        let first_pass_texture = self.first_pass_texture.get(size);
        let output_pass_texture = self.output_pass_texture.get(size);

        let first_pass_texture_view = first_pass_texture.create_view(&Default::default());
        let output_pass_texture_view = output_pass_texture.create_view(&Default::default());

        let ndc_per_px = [2.0 / size.width as f32, 2.0 / size.height as f32];
        let ndc_per_em = [
            ndc_per_px[0] * params.px_per_em,
            ndc_per_px[1] * params.px_per_em,
        ];

        // === ATLAS-BASED RENDERING ===
        // Step 1: Build/update the curve atlas with unique glyph curves.
        // We use the Arc pointer as a key to identify unique glyph shapes.
        let mut atlas_data: Vec<AtlasCurve> = Vec::new();
        let mut new_atlas_index: HashMap<usize, (u32, u32)> = HashMap::new();

        // First pass: collect all unique curve sets from visible glyphs
        for glyph in params.glyphs.iter() {
            let arc_ptr = Arc::as_ptr(&glyph.curves) as usize;
            if !new_atlas_index.contains_key(&arc_ptr) {
                let start_idx = atlas_data.len() as u32;
                for &[p0, p1, p2] in glyph.curves.iter() {
                    atlas_data.push(AtlasCurve {
                        p0,
                        p1,
                        p2,
                        _padding: [0.0, 0.0],
                    });
                }
                let count = glyph.curves.len() as u32;
                new_atlas_index.insert(arc_ptr, (start_idx, count));
            }
        }

        // Step 2: Build instance buffer with curve references and offsets.
        let mut instance_data: Vec<CurveInstance> = Vec::new();
        let mut visible_glyph_count = 0;

        for glyph in params.glyphs.iter() {
            let glyph_x = glyph.offset[0];
            let glyph_y = glyph.offset[1];

            // Viewport culling
            if glyph_x >= viewport_min_x && glyph_x <= viewport_max_x &&
               glyph_y >= viewport_min_y && glyph_y <= viewport_max_y {
                visible_glyph_count += 1;

                let arc_ptr = Arc::as_ptr(&glyph.curves) as usize;
                let (start_idx, count) = new_atlas_index[&arc_ptr];

                // Check curve limit
                if instance_data.len() + count as usize > MAX_CURVES_PER_FRAME {
                    stats.curve_limited = true;
                    break;
                }

                // Create an instance for each curve in this glyph
                for i in 0..count {
                    instance_data.push(CurveInstance {
                        curve_index: start_idx + i,
                        _padding: 0,
                        offset: glyph.offset,
                    });
                }
            }
        }

        stats.visible_glyphs = visible_glyph_count;
        stats.rendered_curves = instance_data.len();
        stats.unique_curves = atlas_data.len();

        let instance_count = instance_data.len() as u32;

        if instance_count == 0 {
            return (self.gfx.create_dummy_texture_view(), stats);
        }

        // Upload atlas data
        let curve_atlas_buffer = self.curve_atlas_buffer.with_data(&atlas_data);
        // Upload instance data
        let curve_instance_buffer = self.curve_instance_buffer.with_data(&instance_data);
        // Invalidate bind group since buffers may have been reallocated
        self.atlas_bind_group = None;
        self.atlas_index = new_atlas_index;

        // Prepare first pass uniform data with dirty checking.
        let current_params = (params.px_per_em, ndc_per_em, params.translation);
        let first_pass_uniform_buffer = if self.last_first_pass_params.as_ref() != Some(&current_params) {
            // Generate uniform data for all 6 samples.
            let first_pass_uniform_data: Vec<FirstPassUniform> = SAMPLES
                .iter()
                .map(|(sample_offset, components)| FirstPassUniform {
                    components: *components,
                    scale: ndc_per_em,
                    translation: [
                        params.translation[0] + sample_offset[0] / params.px_per_em,
                        params.translation[1] + sample_offset[1] / params.px_per_em,
                    ],
                })
                .collect();

            self.last_first_pass_params = Some(current_params);
            // Invalidate bind group since buffer may have been reallocated.
            self.atlas_bind_group = None;

            self.first_pass_uniform_buffer.with_data(&first_pass_uniform_data)
        } else {
            self.first_pass_uniform_buffer.get(SAMPLE_COUNT as usize)
        };

        // Prepare output pass uniform data with dirty checking.
        let output_pass_uniform_data = OutputPassUniform {
            sample_count: SAMPLE_COUNT,
            subpixel_aa: params.subpixel_aa as u32,
            gamma: params.gamma,
        };

        if self.last_output_pass_uniform.as_ref() != Some(&output_pass_uniform_data) {
            self.gfx.queue.write_buffer(
                &self.output_pass_uniform_buffer,
                0,
                bytemuck::bytes_of(&output_pass_uniform_data),
            );
            self.last_output_pass_uniform = Some(output_pass_uniform_data);
        }

        // Create or reuse atlas bind group (includes uniform + atlas storage buffer).
        if self.atlas_bind_group.is_none() {
            self.atlas_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("teqxt_atlas_bind_group"),
                layout: &self.pipelines.atlas_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: UNIFORM_BINDING,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &first_pass_uniform_buffer,
                            offset: 0,
                            size: Some(NonZeroU64::new(FirstPassUniform::WGPU_SIZE).unwrap()),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: CURVE_ATLAS_BINDING,
                        resource: curve_atlas_buffer.as_entire_binding(),
                    },
                ],
            }));
        }
        let atlas_bind_group = self.atlas_bind_group.as_ref().unwrap();

        // Create or reuse output pass bind group (invalidate when texture size changes).
        let needs_new_output_bind_group = match &self.output_pass_bind_group {
            Some((cached_size, _)) => *cached_size != size,
            None => true,
        };
        if needs_new_output_bind_group {
            self.output_pass_bind_group = Some((
                size,
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("teqxt_output_pass_bind_group"),
                    layout: &self.pipelines.render_output.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: UNIFORM_BINDING,
                            resource: self.output_pass_uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: SAMPLE_TEXTURE_BINDING,
                            resource: wgpu::BindingResource::TextureView(&first_pass_texture_view),
                        },
                    ],
                }),
            ));
        }
        let output_pass_bind_group = &self.output_pass_bind_group.as_ref().unwrap().1;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("teqxt_render_encoder"),
        });

        // First render pass: accumulate samples using atlas-based rendering.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("teqxt_first_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &first_pass_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_vertex_buffer(0, curve_instance_buffer.slice(..));

            // Render triangles for all 6 samples using atlas pipeline.
            render_pass.set_pipeline(&self.pipelines.atlas_triangles);
            for i in 0..SAMPLE_COUNT {
                let uniform_buffer_offset = i * FirstPassUniform::WGPU_STRIDE as u32;
                render_pass.set_bind_group(0, atlas_bind_group, &[uniform_buffer_offset]);
                render_pass.draw(0..3, 0..instance_count);
            }

            // Render beziers for all 6 samples using atlas pipeline.
            render_pass.set_pipeline(&self.pipelines.atlas_beziers);
            for i in 0..SAMPLE_COUNT {
                let uniform_buffer_offset = i * FirstPassUniform::WGPU_STRIDE as u32;
                render_pass.set_bind_group(0, atlas_bind_group, &[uniform_buffer_offset]);
                render_pass.draw(0..3, 0..instance_count);
            }
        }

        // Output render pass: resolve samples to final colors.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("teqxt_output_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_pass_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.pipelines.render_output);
            render_pass.set_bind_group(0, output_pass_bind_group, &[]);
            render_pass.draw(0..4, 0..1);
        }

        self.gfx.queue.submit([encoder.finish()]);

        (output_pass_texture_view, stats)
    }
}
