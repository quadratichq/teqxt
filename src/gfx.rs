use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicBool, Ordering},
};

use itertools::Itertools;
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};

const SAMPLE_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const UNIFORM_BINDING: u32 = 0;
const UNIFORM_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry = wgpu::BindGroupLayoutEntry {
    binding: UNIFORM_BINDING,
    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
    ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: true,
        min_binding_size: None,
    },
    count: None,
};

const CURVE_DATA_BINDING: u32 = 1;
const CURVE_DATA_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry = wgpu::BindGroupLayoutEntry {
    binding: CURVE_DATA_BINDING,
    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
    ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    },
    count: None,
};

const SAMPLE_TEXTURE_BINDING: u32 = 2;
const SAMPLE_TEXTURE_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry = wgpu::BindGroupLayoutEntry {
    binding: SAMPLE_TEXTURE_BINDING,
    visibility: wgpu::ShaderStages::FRAGMENT,
    ty: wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: false },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    },
    count: None,
};

const ADDITIVE_BLENDING: wgpu::BlendComponent = wgpu::BlendComponent {
    src_factor: wgpu::BlendFactor::One,
    dst_factor: wgpu::BlendFactor::One,
    operation: wgpu::BlendOperation::Add,
};

/// Sample locations, based on [a blog post by Evan Wallace][evanwallace].
///
/// [evanwallace]:
///     https://medium.com/@evanwallace/easy-scalable-text-rendering-on-the-gpu-c3f4d782c5ac,
const SAMPLES: &[([f32; 2], [f32; 4])] = {
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 0.0];
    const GREEN_ALPHA: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

    // Add alpha on one sample for metadata. We choose a sample that is near the
    // middle of the pixel.

    &[
        ([0.0 / 6.0, 4.0 / 6.0], BLUE),
        ([1.0 / 6.0, 1.0 / 6.0], BLUE),
        ([2.0 / 6.0, 5.0 / 6.0], GREEN),
        ([3.0 / 6.0, 2.0 / 6.0], GREEN_ALPHA),
        ([4.0 / 6.0, 3.0 / 6.0], RED),
        ([5.0 / 6.0, 0.0 / 6.0], RED),
    ]
};

pub struct Gfx {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,

    pub dirty: AtomicBool,
    pub sample_texture: Mutex<wgpu::Texture>,
    pub output_texture: Mutex<wgpu::Texture>,

    pub bezier_vertex_buffer: Mutex<Option<wgpu::Buffer>>,
    pub uniform_buffer: Mutex<wgpu::Buffer>,

    pub render_triangle_pipeline: wgpu::RenderPipeline,
    pub render_bezier_pipeline: wgpu::RenderPipeline,
    pub render_postprocess_pipeline: wgpu::RenderPipeline,
}
impl Gfx {
    pub fn new(
        _adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let size = wgpu::Extent3d::default(); // 1x1
        let sample_texture =
            Self::create_texture(&device, "sample_texture", SAMPLE_TEXTURE_FORMAT, size);
        let output_texture = Self::create_texture(&device, "output_texture", target_format, size);

        let shader_module = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let render_triangle_pipeline = Self::create_render_samples_pipeline(
            &device,
            &shader_module,
            "render_triangle_pipeline",
            "triangle_vertex",
            "triangle_fragment",
            wgpu::FrontFace::Cw,
        );
        let render_bezier_pipeline = Self::create_render_samples_pipeline(
            &device,
            &shader_module,
            "render_bezier_pipeline",
            "bezier_vertex",
            "bezier_fragment",
            wgpu::FrontFace::Ccw,
        );

        let label = "render_postprocess_pipeline";
        let render_postprocess_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(
                    &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some(&format!("{label}_layout")),
                        bind_group_layouts: &[&device.create_bind_group_layout(
                            &wgpu::BindGroupLayoutDescriptor {
                                label: Some(&format!("{label}_bind_group_layout")),
                                entries: &[SAMPLE_TEXTURE_BINDING_LAYOUT],
                            },
                        )],
                        push_constant_ranges: &[],
                    }),
                ),
                vertex: wgpu::VertexState {
                    module: &shader_module,
                    entry_point: Some("postprocess_vertex"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader_module,
                    entry_point: Some("postprocess_fragment"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING), // TODO: maybe not this?
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_buffer"),
            size: Uniform::WGPU_STRIDE as u64 * SAMPLES.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });

        Self {
            device,
            queue,
            target_format,

            dirty: AtomicBool::from(true),
            sample_texture: Mutex::new(sample_texture),
            output_texture: Mutex::new(output_texture),

            bezier_vertex_buffer: Mutex::new(None),
            uniform_buffer: Mutex::new(uniform_buffer),

            render_triangle_pipeline,
            render_bezier_pipeline,
            render_postprocess_pipeline,
        }
    }

    fn create_render_samples_pipeline(
        device: &wgpu::Device,
        shader_module: &wgpu::ShaderModule,
        label: &str,
        vertex_entry_point: &str,
        fragment_entry_point: &str,
        front_face: wgpu::FrontFace,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(&format!("{label}_layout")),
                    bind_group_layouts: &[&device.create_bind_group_layout(
                        &wgpu::BindGroupLayoutDescriptor {
                            label: Some(&format!("{label}_bind_group_layout")),
                            entries: &[UNIFORM_BINDING_LAYOUT, CURVE_DATA_BINDING_LAYOUT],
                        },
                    )],
                    push_constant_ranges: &[],
                }),
            ),
            vertex: wgpu::VertexState {
                module: shader_module,
                entry_point: Some(vertex_entry_point),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader_module,
                entry_point: Some(fragment_entry_point),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SAMPLE_TEXTURE_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: ADDITIVE_BLENDING,
                        alpha: wgpu::BlendComponent::REPLACE, // use alpha channel for extra info, not samples
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        })
    }

    /// Resizes the bezier vertex buffer to the given length and locks its
    /// mutex.
    pub fn lock_bezier_vertex_buffer(&self, len: usize) -> MappedMutexGuard<'_, wgpu::Buffer> {
        let desired_size = std::cmp::max(len as u64, 1) * BezierCurve::WGPU_STRIDE;
        self.resize_and_lock_buffer(
            Some("bezier_vertex_buffer"),
            &self.bezier_vertex_buffer,
            desired_size,
        )
    }

    fn resize_and_lock_buffer<'a>(
        &self,
        label: Option<&str>,
        buffer: &'a Mutex<Option<wgpu::Buffer>>,
        desired_size: u64,
    ) -> MappedMutexGuard<'a, wgpu::Buffer> {
        MutexGuard::map(buffer.lock(), |guard| {
            if guard.as_ref().is_some_and(|buf| buf.size() != desired_size) {
                *guard = None;
            }
            guard.get_or_insert_with(|| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label,
                    size: desired_size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            })
        })
    }

    pub fn set_output_size(&self, width: u32, height: u32) {
        let new_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let mut output_texture = self.output_texture.lock();
        if output_texture.size() != new_size {
            self.dirty.store(true, Ordering::Relaxed);
            *self.sample_texture.lock() = Self::create_texture(
                &self.device,
                "sample_texture",
                SAMPLE_TEXTURE_FORMAT,
                new_size,
            );
            *output_texture =
                Self::create_texture(&self.device, "output_texture", self.target_format, new_size);
        }
    }

    fn output_texture_view(&self) -> wgpu::TextureView {
        self.output_texture.lock().create_view(&Default::default())
    }

    fn create_texture(
        device: &wgpu::Device,
        label: &str,
        format: wgpu::TextureFormat,
        size: wgpu::Extent3d,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }
}

pub struct DrawParams {
    pub scale: [f32; 2],
    pub translation: [f32; 2],
    pub glyphs: Vec<Glyph>,
}

/// Uniform buffer data.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct Uniform {
    pub scale: [f32; 2],
    pub translation: [f32; 2],
    pub components: [f32; 4],
}
impl Uniform {
    const WGPU_SIZE: u64 = std::mem::size_of::<Self>() as u64;
    const WGPU_STRIDE: u32 = wgpu::Limits::downlevel_defaults().min_uniform_buffer_offset_alignment;
}

#[derive(Debug, Clone)]
pub struct Glyph {
    pub offset: [f32; 2],
    pub curves: Vec<[[f32; 2]; 3]>,
}

/// Quadratic bezier curve in 2D em space.
///
/// Fields are ordered so that the taking the first 3 gives the flat triangle
/// and taking the last 3 gives the bezier curve.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct BezierCurve {
    pub origin: [f32; 2],
    pub p0: [f32; 2],
    pub p2: [f32; 2],
    pub p1: [f32; 2],
}
impl BezierCurve {
    const WGPU_STRIDE: u64 = std::mem::size_of::<[f32; 2]>() as u64 * 4;
}

pub fn draw(gfx: &Gfx, params: DrawParams) {
    let mut encoder = gfx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("teqxt_render_encoder"),
        });

    let curves = params
        .glyphs
        .iter()
        .flat_map(|glyph| {
            let origin = glyph.offset;
            let curves = glyph.curves.iter();
            curves.map(move |&[p0, p1, p2]| BezierCurve { origin, p0, p2, p1 })
        })
        .collect_vec();
    let vertex_buffer = gfx.lock_bezier_vertex_buffer(curves.len());
    gfx.queue
        .write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&curves));

    let output_size = gfx.output_texture.lock().size();
    let pixel_width = 2.0 / output_size.width as f32;
    let pixel_height = 2.0 / output_size.height as f32;

    for (i, &(offset, components)) in SAMPLES.iter().enumerate() {
        gfx.queue.write_buffer(
            &gfx.uniform_buffer.lock(),
            i as u64 * Uniform::WGPU_STRIDE as u64,
            bytemuck::bytes_of(&Uniform {
                scale: params.scale,
                translation: [
                    (params.translation[0] * params.scale[0] / pixel_width).floor() * pixel_width
                        + offset[0] * pixel_width,
                    (params.translation[1] * params.scale[0] / pixel_height).floor() * pixel_height
                        + offset[1] * pixel_height,
                ],
                components,
            }),
        );
    }

    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("teqxt_main_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &gfx.sample_texture.lock().create_view(&Default::default()),
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

        let bind_group = gfx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("teqxt_main_render_pass_bind_group"),
            layout: &gfx.render_triangle_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: UNIFORM_BINDING,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &gfx.uniform_buffer.lock(),
                        offset: 0,
                        size: Some(NonZeroU64::new(Uniform::WGPU_SIZE).unwrap()),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: CURVE_DATA_BINDING,
                    resource: vertex_buffer.as_entire_binding(),
                },
            ],
        });

        let vertex_count = curves.len() as u32 * 3;

        render_pass.set_pipeline(&gfx.render_triangle_pipeline);
        for i in 0..SAMPLES.len() as u32 {
            render_pass.set_bind_group(0, &bind_group, &[i * Uniform::WGPU_STRIDE]);
            render_pass.draw(0..vertex_count, 0..1);
        }

        render_pass.set_pipeline(&gfx.render_bezier_pipeline);
        for i in 0..SAMPLES.len() as u32 {
            render_pass.set_bind_group(0, &bind_group, &[i * Uniform::WGPU_STRIDE]);
            render_pass.draw(0..vertex_count, 0..1);
        }
    }

    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("teqxt_postprocess_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &gfx.output_texture_view(),
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

        render_pass.set_bind_group(
            0,
            &gfx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("teqxt_postprocess_render_pass_bind_group"),
                layout: &gfx.render_postprocess_pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: SAMPLE_TEXTURE_BINDING,
                    resource: wgpu::BindingResource::TextureView(
                        &gfx.sample_texture.lock().create_view(&Default::default()),
                    ),
                }],
            }),
            &[],
        );

        render_pass.set_pipeline(&gfx.render_postprocess_pipeline);
        render_pass.draw(0..4, 0..1);
    }

    gfx.queue.submit([encoder.finish()]);
}
