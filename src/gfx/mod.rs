mod bindings;
mod cached;
mod pipelines;
mod renderer;
mod structs;

pub use renderer::{DrawParams, Glyph, Renderer};

const SAMPLE_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Graphics driver state.
///
/// This type is relatively cheap to clone.
#[derive(Clone)]
pub struct Gfx {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,

    pub dummy_texture: wgpu::Texture,
}
impl Gfx {
    pub fn new(
        _adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dummy_texture"),
            size: wgpu::Extent3d::default(), // 1x1
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: target_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        Self {
            device,
            queue,
            target_format,

            dummy_texture,
        }
    }

    pub fn create_dummy_texture_view(&self) -> wgpu::TextureView {
        self.dummy_texture.create_view(&Default::default())
    }
}
