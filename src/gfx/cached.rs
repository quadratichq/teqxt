use std::{marker::PhantomData, sync::Arc};

use super::{Gfx, structs::WgpuStruct};

/// Cached buffer that is automatically resized as needed.
pub struct CachedBuffer<T> {
    inner: Cached<usize, wgpu::Buffer>,
    marker: PhantomData<T>,
}
impl<T: WgpuStruct> CachedBuffer<T> {
    /// Constructs a new cached buffer.
    pub fn new(gfx: &Gfx, label: &str, usage: wgpu::BufferUsages) -> Self {
        let label = label.to_owned();
        Self {
            inner: Cached::new(gfx, move |gfx, len| {
                gfx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&label),
                    size: T::WGPU_STRIDE * len as u64,
                    usage,
                    mapped_at_creation: false,
                })
            }),
            marker: PhantomData,
        }
    }

    pub fn get(&mut self, len: usize) -> Arc<wgpu::Buffer> {
        self.inner.get(len)
    }
    /// Creates the buffer and fills it with the given data.
    ///
    /// The buffer itself is reused, but the data is always rewritten.
    pub fn with_data(&mut self, data: &[T]) -> Arc<wgpu::Buffer> {
        let buffer = self.get(data.len());
        if T::WGPU_STRIDE == std::mem::size_of::<T>() as u64 {
            // Optimization: copy the bytes of the array directly.
            let bytes = bytemuck::cast_slice(data);
            self.inner.gfx.queue.write_buffer(&buffer, 0, bytes);
        } else {
            // General case: copy each individual element of the array.
            for (i, elem) in data.iter().enumerate() {
                let offset = T::WGPU_STRIDE * i as u64;
                let bytes = bytemuck::bytes_of(elem);
                self.inner.gfx.queue.write_buffer(&buffer, offset, bytes);
            }
        }
        buffer
    }
}

/// Object of type `T` cached using a key of type `K`.
///
/// For example, `T` may be [`wgpu::Texture`] and `K` may be [`wgpu::Extent3d`]
/// for a texture that is automatically resized as needed.
pub struct Cached<K, T> {
    gfx: Gfx,
    cached: Option<(K, Arc<T>)>,
    f: Box<dyn Fn(&Gfx, K) -> T>,
}
impl<K: Clone + Eq, T> Cached<K, T> {
    /// Constructs a new cached object, given a function to create it when
    /// needed.
    pub fn new(gfx: &Gfx, f: impl 'static + Fn(&Gfx, K) -> T) -> Self {
        Self {
            gfx: gfx.clone(),
            cached: None,
            f: Box::new(f),
        }
    }
    /// Returns the cached object, invalidating and recreating it if `key` is
    /// different from the key that was used previously.
    pub fn get(&mut self, key: K) -> Arc<T> {
        self.cached.take_if(|(old_key, _)| *old_key != key);
        let (_key, obj) = self
            .cached
            .get_or_insert_with(|| (key.clone(), Arc::new((self.f)(&self.gfx, key))));
        Arc::clone(obj)
    }

    /// Returns the key that was most recently used to create the object, or
    /// `None` if the object hasn't been created yet.
    pub fn key(&self) -> Option<&K> {
        self.cached.as_ref().map(|(key, _obj)| key)
    }
}
impl<T> Cached<usize, T> {
    /// Returns the cached object, invalidating and recreating it if `key` is
    /// greater than the key that was used previously.
    pub fn get_at_least(&mut self, mut key: usize) -> Arc<T> {
        if let Some(&old_key) = self.key().filter(|&&old_key| old_key > key) {
            key = old_key;
        }
        self.get(key)
    }
}
