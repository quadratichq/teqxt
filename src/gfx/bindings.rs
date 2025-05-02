pub const UNIFORM_BINDING: u32 = 0;
pub const FIRST_PASS_UNIFORM_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry =
    wgpu::BindGroupLayoutEntry {
        binding: UNIFORM_BINDING,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: None,
        },
        count: None,
    };
pub const OUTPUT_PASS_UNIFORM_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry =
    wgpu::BindGroupLayoutEntry {
        binding: UNIFORM_BINDING,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };

pub const SAMPLE_TEXTURE_BINDING: u32 = 1;
pub const SAMPLE_TEXTURE_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry = wgpu::BindGroupLayoutEntry {
    binding: SAMPLE_TEXTURE_BINDING,
    visibility: wgpu::ShaderStages::FRAGMENT,
    ty: wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: false },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    },
    count: None,
};
