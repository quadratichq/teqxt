use std::mem::size_of;

/// Returns the WGPU size for a struct `T`, padded to the length of a WGPU
/// vector type with length `align_vec_size`.
///
/// For example, if `align_vec_size` is `4`, then the size of `T` is padded to
/// the next `vec4<f32>`.
const fn wgpu_align<T>(align_vec_size: usize) -> u64 {
    size_of::<T>().next_multiple_of(size_of::<f32>() * align_vec_size) as u64
}

/// Trait for structs that can be converted to/from bytes and sent to/from the GPU.
pub trait WgpuStruct: bytemuck::NoUninit {
    /// Size of the type in WGPU.
    ///
    /// This must be at least as large as the size of the struct, and must be
    /// aligned to the struct's WGPU alignment.
    const WGPU_SIZE: u64;

    /// Stride for arrays of the struct in WGPU.
    ///
    /// This must be at least as large as the size of the struct, and must be
    /// aligned to the struct's WGPU alignment. It may be larger.
    const WGPU_STRIDE: u64;
}

/// Quadratic bezier curve in 2D em space (OLD - kept for compatibility).
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct BezierCurveInstance {
    /// Global offset for the glyph.
    pub offset: [f32; 2],
    /// Start point, relative to `offset`.
    pub p0: [f32; 2],
    /// Middle control point, relative to `offset`.
    pub p1: [f32; 2],
    /// End point, relative to `offset`.
    pub p2: [f32; 2],
}
impl WgpuStruct for BezierCurveInstance {
    const WGPU_SIZE: u64 = wgpu_align::<Self>(2);
    const WGPU_STRIDE: u64 = Self::WGPU_SIZE;
}
impl BezierCurveInstance {
    pub const VERTEX_BUFFER_LAYOUT: wgpu::VertexBufferLayout<'_> = wgpu::VertexBufferLayout {
        array_stride: Self::WGPU_STRIDE,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x2, // offset
            1 => Float32x2, // p0
            2 => Float32x2, // p1
            3 => Float32x2, // p2
        ],
    };
}

/// Curve data stored in the glyph atlas (local space, no offset).
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct AtlasCurve {
    /// Start point.
    pub p0: [f32; 2],
    /// Middle control point.
    pub p1: [f32; 2],
    /// End point.
    pub p2: [f32; 2],
    /// Padding for alignment.
    pub _padding: [f32; 2],
}
impl WgpuStruct for AtlasCurve {
    const WGPU_SIZE: u64 = wgpu_align::<Self>(4);
    const WGPU_STRIDE: u64 = Self::WGPU_SIZE;
}

/// Instance data for a single curve (references atlas + provides offset).
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct CurveInstance {
    /// Index of the curve in the atlas.
    pub curve_index: u32,
    /// Padding for alignment.
    pub _padding: u32,
    /// Global offset for the glyph.
    pub offset: [f32; 2],
}
impl WgpuStruct for CurveInstance {
    const WGPU_SIZE: u64 = wgpu_align::<Self>(4);
    const WGPU_STRIDE: u64 = Self::WGPU_SIZE;
}
impl CurveInstance {
    pub const VERTEX_BUFFER_LAYOUT: wgpu::VertexBufferLayout<'_> = wgpu::VertexBufferLayout {
        array_stride: Self::WGPU_STRIDE,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Uint32,    // curve_index
            1 => Uint32,    // padding (unused)
            2 => Float32x2, // offset
        ],
    };
}

/// Glyph instance data for batched rendering - just the offset.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct GlyphOffset {
    /// Global offset for the glyph instance.
    pub offset: [f32; 2],
}
impl WgpuStruct for GlyphOffset {
    const WGPU_SIZE: u64 = wgpu_align::<Self>(2);
    const WGPU_STRIDE: u64 = Self::WGPU_SIZE;
}
impl GlyphOffset {
    pub const VERTEX_BUFFER_LAYOUT: wgpu::VertexBufferLayout<'_> = wgpu::VertexBufferLayout {
        array_stride: Self::WGPU_STRIDE,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x2, // offset
        ],
    };
}

/// Uniform data for batched glyph rendering.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, PartialEq, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct BatchUniform {
    /// Sample components (RGBA mask).
    pub components: [f32; 4],
    /// Scale to convert from ems to NDC.
    pub scale: [f32; 2],
    /// Translation in ems.
    pub translation: [f32; 2],
    /// Start index in the curve atlas for this glyph type.
    pub curve_start: u32,
    /// Number of curves in this glyph type.
    pub curve_count: u32,
    /// Padding for alignment.
    pub _padding: [u32; 2],
}
impl WgpuStruct for BatchUniform {
    const WGPU_SIZE: u64 = wgpu_align::<Self>(4);
    const WGPU_STRIDE: u64 = Self::WGPU_SIZE;
}

/// Uniform data for the first pass.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, PartialEq, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct FirstPassUniform {
    /// Components to write to (RGBA, each is 1 or 0).
    pub components: [f32; 4],
    /// Global scale to apply to convert from ems to NDC (normalized device
    /// coordinates).
    pub scale: [f32; 2],
    /// Global translation to apply before scale, in ems.
    pub translation: [f32; 2],
}
impl WgpuStruct for FirstPassUniform {
    const WGPU_SIZE: u64 = wgpu_align::<Self>(4);
    // We store several of these in an array and use an offset to select a
    // different one for each draw call, so the array has to be padded to the
    // `min_uniform_buffer_offset_alignment`.
    const WGPU_STRIDE: u64 =
        wgpu::Limits::downlevel_defaults().min_uniform_buffer_offset_alignment as u64;
}

/// Uniform data for the output pass.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, PartialEq, bytemuck::NoUninit, bytemuck::Zeroable)]
pub struct OutputPassUniform {
    /// Number of samples per pixel done in the first pass.
    pub sample_count: u32,
    /// Whether to enable subpixel anti-aliasing (0 = off, 1 = on).
    pub subpixel_aa: u32,
    /// Gamma value (typically 2.2).
    pub gamma: f32, // TODO: do sRGB properly instead of a gamma value
}
impl WgpuStruct for OutputPassUniform {
    const WGPU_SIZE: u64 = wgpu_align::<Self>(2);
    const WGPU_STRIDE: u64 = Self::WGPU_SIZE;
}
