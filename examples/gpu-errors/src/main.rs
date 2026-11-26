//! Says where wgpu's errors reach it, one line each, as `what: where`, as wgpu's own backends
//! route them: an error made once a scope is popped, before its future is awaited, goes to the
//! scope outside it; and one made once a scope's guard is dropped unawaited goes to the device's
//! uncaptured-error handler, the scope being gone.
//!
//! ```text
//! cargo build -p gpu-errors --target wasm32-wasip3
//! ```

use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let device = device().await?;
    let (uncaptured, mut heard) = mpsc::unbounded();
    device.on_uncaptured_error(Arc::new(move |error| {
        let _ = uncaptured.unbounded_send(kind(Some(&error)));
    }));

    let outer = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let inner = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let inner_popped = inner.pop();
    invalid_buffer(&device);
    let inner_caught = inner_popped.await;
    println!(
        "made after the inner scope's pop, the inner scope caught: {}",
        kind(inner_caught.as_ref())
    );
    println!("and the outer scope: {}", kind(outer.pop().await.as_ref()));

    drop(device.push_error_scope(wgpu::ErrorFilter::Validation));
    invalid_buffer(&device);
    let reached = heard.next().await.unwrap_or("nothing");
    println!("made after a scope's guard was dropped, the handler heard: {reached}");
    Ok(())
}

async fn device() -> Result<wgpu::Device, String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no adapter: {error}"))?;
    let (device, _queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|error| format!("no device: {error}"))?;
    Ok(device)
}

/// A buffer both mapped for reading and for writing, which wgpu-core refuses as invalid.
fn invalid_buffer(device: &wgpu::Device) {
    let _invalid = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
        mapped_at_creation: false,
    });
}

fn kind(error: Option<&wgpu::Error>) -> &'static str {
    match error {
        None => "none",
        Some(wgpu::Error::OutOfMemory { .. }) => "out of memory",
        Some(wgpu::Error::Validation { .. }) => "validation",
        Some(wgpu::Error::Internal { .. }) => "internal",
    }
}
