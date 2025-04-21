use std::sync::atomic::{AtomicBool, Ordering};

use egui::mutex::Mutex;

pub struct Gfx {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,

    pub dirty: AtomicBool,
    pub output_texture: Mutex<wgpu::Texture>,

    pub vertex_buffer: Mutex<wgpu::Buffer>,
    pub uniform_buffer: Mutex<wgpu::Buffer>,

    pub render_triangle_pipeline: wgpu::RenderPipeline,
}
impl Gfx {
    pub fn new(
        _adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let size = wgpu::Extent3d::default(); // 1x1
        let output_texture = Self::create_output_texture(&device, target_format, size);

        let shader_module = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let render_triangle_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("render_triangle_pipeline"),
                layout: Some(
                    &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("render_triangle_pipeline_layout"),
                        bind_group_layouts: &[&device.create_bind_group_layout(
                            &wgpu::BindGroupLayoutDescriptor {
                                label: Some("render_triangle_pipeline_bind_group_layout"),
                                entries: &[wgpu::BindGroupLayoutEntry {
                                    binding: 0,
                                    visibility: wgpu::ShaderStages::VERTEX,
                                    ty: wgpu::BindingType::Buffer {
                                        ty: wgpu::BufferBindingType::Uniform,
                                        has_dynamic_offset: false,
                                        min_binding_size: None,
                                    },
                                    count: None,
                                }],
                            },
                        )],
                        push_constant_ranges: &[],
                    }),
                ),
                vertex: wgpu::VertexState {
                    module: &shader_module,
                    entry_point: Some("vertex"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x2],
                    }],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
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
                    entry_point: Some("fragment"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vertex_buffer"),
            size: 8 * 3,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_buffer"),
            size: 8,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });

        Self {
            device,
            queue,
            target_format,

            dirty: AtomicBool::from(true),
            output_texture: Mutex::new(output_texture),

            vertex_buffer: Mutex::new(vertex_buffer),
            uniform_buffer: Mutex::new(uniform_buffer),

            render_triangle_pipeline,
        }
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
            *output_texture =
                Self::create_output_texture(&self.device, self.target_format, new_size);
        }
    }

    fn output_texture_view(&self) -> wgpu::TextureView {
        self.output_texture
            .lock()
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn create_output_texture(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: wgpu::Extent3d,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output_texture"),
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
    pub points: [[f32; 2]; 3],
}

pub fn draw(gfx: &Gfx, params: DrawParams) {
    let mut encoder = gfx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("teqxt_render_encoder"),
        });

    gfx.queue.write_buffer(
        &gfx.vertex_buffer.lock(),
        0,
        bytemuck::bytes_of(&params.points),
    );

    gfx.queue.write_buffer(
        &gfx.uniform_buffer.lock(),
        0,
        bytemuck::bytes_of(&params.scale),
    );

    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("teqxt_main_render_pass"),
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

        render_pass.set_pipeline(&gfx.render_triangle_pipeline);

        render_pass.set_vertex_buffer(0, gfx.vertex_buffer.lock().slice(..));
        render_pass.set_bind_group(
            0,
            &gfx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("uniform_bind_group"),
                layout: &gfx.render_triangle_pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: gfx.uniform_buffer.lock().as_entire_binding(),
                }],
            }),
            &[],
        );

        render_pass.draw(0..3, 0..1);
    }

    gfx.queue.submit([encoder.finish()]);
}
