//! The program's GPU device: one adapter, one device and its queue, for the scene and the panel.

/// The device everything is drawn with, and what it was made from.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Asks for the default adapter and a device with WebGPU's default limits, which is all the
    /// renderer needs.
    pub async fn request() -> Result<Gpu, String> {
        let instance = wasdk_wgpu::instance();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|error| format!("no WebGPU adapter: {error}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("model viewer"),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("no WebGPU device: {error}"))?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }
}
