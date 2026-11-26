//! Says where wgpu's errors reach it, one line each, as `what: where`, as wgpu's own backends
//! route them: an error made once a scope is popped, before its future is awaited, goes to the
//! scope outside it; and one made once a scope's guard is dropped unawaited goes to the device's
//! uncaptured-error handler, the scope being gone. Then says where a device's loss goes: to the
//! device-lost callback set last, one set before it dropped uncalled; and a device dropped
//! without being lost drops its callback uncalled.
//!
//! ```text
//! cargo build -p gpu-errors --target wasm32-wasip3
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use futures::channel::mpsc;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let (device, _queue) = device_and_queue().await?;
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

    let (replaced, replaced_dropped) = Dropped::new();
    device.set_device_lost_callback(move |_, _| {
        let _held = &replaced;
    });
    let (told, mut heard_lost) = mpsc::unbounded();
    device.set_device_lost_callback(move |reason, _| {
        let _ = told.unbounded_send(reason);
    });
    println!(
        "a device-lost callback replaced is dropped uncalled: {}",
        replaced_dropped.is_set()
    );
    device.destroy();
    let reason = heard_lost.next().await;
    println!("the callback set last heard: {reason:?}");

    let unlost = device_and_queue().await?.0;
    let (callback, callback_dropped) = Dropped::new();
    unlost.set_device_lost_callback(move |_, _| {
        let _held = &callback;
    });
    drop(unlost);
    for _ in 0..10 {
        if callback_dropped.is_set() {
            break;
        }
        wasdk::yield_async().await;
    }
    println!(
        "a device dropped unlost drops its callback uncalled: {}",
        callback_dropped.is_set()
    );
    Ok(())
}

/// What a callback holds, which says once it is dropped.
struct Dropped(Arc<AtomicBool>);

/// Whether a [`Dropped`] went.
struct DroppedFlag(Arc<AtomicBool>);

impl Dropped {
    fn new() -> (Dropped, DroppedFlag) {
        let flag = Arc::new(AtomicBool::new(false));
        (Dropped(Arc::clone(&flag)), DroppedFlag(flag))
    }
}

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl DroppedFlag {
    fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

async fn device_and_queue() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no adapter: {error}"))?;
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|error| format!("no device: {error}"))
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
