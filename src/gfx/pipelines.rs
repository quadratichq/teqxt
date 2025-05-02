use super::{Gfx, SAMPLE_TEXTURE_FORMAT, bindings::*, structs::BezierCurveInstance};

pub struct Pipelines {
    /// Render pipeline for rendering triangles during the first pass.
    pub render_triangles: wgpu::RenderPipeline,
    /// Render pipeline for rendering cubic beziers during the first pass.
    pub render_beziers: wgpu::RenderPipeline,
    /// Render pipeline for the output pass.
    pub render_output: wgpu::RenderPipeline,
}
impl Pipelines {
    pub fn new(gfx: &Gfx) -> Self {
        let module = gfx
            .device
            .create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        Self {
            render_triangles: first_pass_pipeline(
                &gfx.device,
                &module,
                "render_triangle_pipeline",
                "triangle_vertex",
                "triangle_fragment",
                wgpu::FrontFace::Cw,
            ),
            render_beziers: first_pass_pipeline(
                &gfx.device,
                &module,
                "render_bezier_pipeline",
                "bezier_vertex",
                "bezier_fragment",
                wgpu::FrontFace::Cw,
            ),
            render_output: output_pass_pipeline(&gfx.device, gfx.target_format, &module),
        }
    }
}

fn first_pass_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
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
                        entries: &[FIRST_PASS_UNIFORM_BINDING_LAYOUT],
                    },
                )],
                push_constant_ranges: &[],
            }),
        ),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex_entry_point),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[BezierCurveInstance::VERTEX_BUFFER_LAYOUT],
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
            module,
            entry_point: Some(fragment_entry_point),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: SAMPLE_TEXTURE_FORMAT,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::REPLACE, // use alpha channel for extra info, not samples
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

fn output_pass_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    module: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    let label = "render_postprocess_pipeline";
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("{label}_layout")),
                bind_group_layouts: &[&device.create_bind_group_layout(
                    &wgpu::BindGroupLayoutDescriptor {
                        label: Some(&format!("{label}_bind_group_layout")),
                        entries: &[
                            OUTPUT_PASS_UNIFORM_BINDING_LAYOUT,
                            SAMPLE_TEXTURE_BINDING_LAYOUT,
                        ],
                    },
                )],
                push_constant_ranges: &[],
            }),
        ),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("output_vertex"),
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
            module,
            entry_point: Some("output_fragment"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING), // TODO: maybe not this?
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}
