//! Opens a GPU device inside `wasdk::block_on`, as a winit app opens one inside its event loop,
//! sets its device-lost callback, submits once, and says `returned` once the wait has: opening a
//! device starts no task that would keep the wait from returning.
//!
//! ```text
//! cargo build -p gpu-in-block-on --target wasm32-wasip3
//! ```

#[wasdk::main]
async fn main() -> Result<(), String> {
    wasdk::block_on(open_and_submit())?;
    println!("returned");
    Ok(())
}

async fn open_and_submit() -> Result<(), String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no WebGPU adapter: {error}"))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            // wgpu's defaults within what the adapter has: some fall below them, as the
            // iOS simulator's does.
            required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
            ..Default::default()
        })
        .await
        .map_err(|error| format!("no WebGPU device: {error}"))?;
    device.set_device_lost_callback(|_, _| {});
    let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    queue.submit([encoder.finish()]);
    Ok(())
}
