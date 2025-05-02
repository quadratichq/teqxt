use std::num::NonZeroU64;

use super::{
    Gfx, SAMPLE_TEXTURE_FORMAT,
    bindings::{SAMPLE_TEXTURE_BINDING, UNIFORM_BINDING},
    cached::*,
    pipelines::Pipelines,
    structs::*,
};

/// Sample locations, based on [a blog post by Evan Wallace][evanwallace].
///
/// [evanwallace]:
///     https://medium.com/@evanwallace/easy-scalable-text-rendering-on-the-gpu-c3f4d782c5ac,
const SAMPLES: [([f32; 2], [f32; 4]); 6] = {
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
    pub glyphs: Vec<Glyph>,
    pub gamma: f32,
    pub subpixel_aa: bool,
}

#[derive(Debug, Clone)]
pub struct Glyph {
    /// XY offset of the glyph, measured in ems.
    pub offset: [f32; 2],
    /// Bezier curve data for the glyph, measured in ems.
    pub curves: Vec<[[f32; 2]; 3]>,
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

    /// Buffer containing Bezier curve data.
    pub bezier_instance_buffer: CachedBuffer<BezierCurveInstance>,
    /// Uniform buffer for the first pass.
    pub first_pass_uniform_buffer: CachedBuffer<FirstPassUniform>,
    /// Uniform buffer for the output pass.
    pub output_pass_uniform_buffer: CachedBuffer<OutputPassUniform>,

    /// Shader pipelines.
    pub pipelines: Pipelines,
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

            bezier_instance_buffer: CachedBuffer::new(
                gfx,
                "bezier_instance_buffer",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            ),
            first_pass_uniform_buffer: CachedBuffer::new(
                gfx,
                "first_pass_uniform_buffer",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            ),
            output_pass_uniform_buffer: CachedBuffer::new(
                gfx,
                "output_pass_uniform_buffer",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            ),

            pipelines: Pipelines::new(gfx),
        }
    }

    pub fn draw(&mut self, params: DrawParams) -> wgpu::TextureView {
        // Avoid crash on resizing texture.
        if params.output_size[0] == 0 || params.output_size[1] == 0 {
            return self.gfx.create_dummy_texture_view();
        }

        let device = &self.gfx.device;
        let mut encoder = self
            .gfx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("teqxt_render_encoder"),
            });

        let size = wgpu::Extent3d {
            width: params.output_size[0],
            height: params.output_size[1],
            depth_or_array_layers: 1,
        };

        let first_pass_texture = self.first_pass_texture.get(size);
        let output_pass_texture = self.output_pass_texture.get(size);

        let first_pass_texture_view = first_pass_texture.create_view(&Default::default());
        let output_pass_texture_view = output_pass_texture.create_view(&Default::default());

        let ndc_per_px = [2.0 / size.width as f32, 2.0 / size.height as f32];
        let ndc_per_em = [
            ndc_per_px[0] * params.px_per_em,
            ndc_per_px[1] * params.px_per_em,
        ];

        // Prepare bezier data.
        let bezier_data: Vec<BezierCurveInstance> = params
            .glyphs
            .iter()
            .flat_map(|glyph| {
                glyph.curves.iter().map(|&[p0, p1, p2]| {
                    let offset = glyph.offset;
                    BezierCurveInstance { offset, p0, p1, p2 }
                })
            })
            .collect();
        let bezier_instance_count = bezier_data.len() as u32;

        // Avoid crash on empty draw call.
        if bezier_instance_count == 0 {
            return self.gfx.create_dummy_texture_view();
        }

        // Prepare uniform data.
        let first_pass_uniform_data = SAMPLES.map(|(sample_offset, components)| FirstPassUniform {
            components,
            scale: ndc_per_em,
            translation: [
                params.translation[0] + sample_offset[0] / params.px_per_em,
                params.translation[1] + sample_offset[1] / params.px_per_em,
            ],
        });
        let output_pass_uniform_data = OutputPassUniform {
            sample_count: SAMPLES.len() as u32,
            subpixel_aa: params.subpixel_aa as u32,
            gamma: params.gamma,
        };

        // Resize and populate buffers.
        let bezier_instance_buffer = self.bezier_instance_buffer.with_data(&bezier_data);
        let first_pass_uniform_buffer = self
            .first_pass_uniform_buffer
            .with_data(&first_pass_uniform_data);
        let output_pass_uniform_buffer = self
            .output_pass_uniform_buffer
            .with_data(&[output_pass_uniform_data]);

        // Do first render pass.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("teqxt_main_render_pass"),
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

            render_pass.set_vertex_buffer(0, bezier_instance_buffer.slice(..));

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("teqxt_main_render_pass_bind_group"),
                layout: &self.pipelines.render_triangles.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: UNIFORM_BINDING,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &first_pass_uniform_buffer,
                        offset: 0,
                        size: Some(NonZeroU64::new(FirstPassUniform::WGPU_SIZE).unwrap()),
                    }),
                }],
            });

            // Render triangles.
            render_pass.set_pipeline(&self.pipelines.render_triangles);
            for i in 0..SAMPLES.len() as u32 {
                let uniform_buffer_offset = i * FirstPassUniform::WGPU_STRIDE as u32;
                render_pass.set_bind_group(0, &bind_group, &[uniform_buffer_offset]);
                render_pass.draw(0..3, 0..bezier_instance_count);
            }

            // Render beziers.
            render_pass.set_pipeline(&self.pipelines.render_beziers);
            for i in 0..SAMPLES.len() as u32 {
                let uniform_buffer_offset = i * FirstPassUniform::WGPU_STRIDE as u32;
                render_pass.set_bind_group(0, &bind_group, &[uniform_buffer_offset]);
                render_pass.draw(0..3, 0..bezier_instance_count);
            }
        }

        // Do output render pass.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("teqxt_postprocess_render_pass"),
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

            render_pass.set_bind_group(
                0,
                &device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("teqxt_postprocess_render_pass_bind_group"),
                    layout: &self.pipelines.render_output.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: UNIFORM_BINDING,
                            resource: output_pass_uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: SAMPLE_TEXTURE_BINDING,
                            resource: wgpu::BindingResource::TextureView(&first_pass_texture_view),
                        },
                    ],
                }),
                &[],
            );

            render_pass.draw(0..4, 0..1);
        }

        self.gfx.queue.submit([encoder.finish()]);

        output_pass_texture_view
    }
}
