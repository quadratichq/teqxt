pub const UNIFORM_BINDING: u32 = 0;

/// Uniform buffer binding for the first pass (with dynamic offset for samples).
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

/// Uniform buffer binding for the output pass.
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

/// Curve atlas storage buffer binding.
pub const CURVE_ATLAS_BINDING: u32 = 2;
pub const CURVE_ATLAS_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry = wgpu::BindGroupLayoutEntry {
    binding: CURVE_ATLAS_BINDING,
    visibility: wgpu::ShaderStages::VERTEX,
    ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    },
    count: None,
};

/// Batch uniform binding (no dynamic offset - we update per batch).
pub const BATCH_UNIFORM_BINDING_LAYOUT: wgpu::BindGroupLayoutEntry = wgpu::BindGroupLayoutEntry {
    binding: UNIFORM_BINDING,
    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
    ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    },
    count: None,
};
