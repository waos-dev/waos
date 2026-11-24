//! Reads a buffer back from the GPU both ways wgpu programs do, and prints what it read.
//!
//! Code written for native wgpu asks for a mapping, calls `Device::poll` with `Wait`, and reads
//! the answer at once: the callback has run before `poll` returns. Code written for the web never
//! polls: it awaits, and the callback runs as the program yields. Both hold on WaOS, and so does
//! the order wgpu promises: a queue's submitted work is told done after the mappings asked before
//! it.

use std::sync::mpsc;

use futures::channel::oneshot;

/// What is written to the GPU and read back.
const WRITTEN: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

#[wasdk::main]
async fn main() -> Result<(), String> {
    let (device, queue) = device().await?;

    let copy = copy_of_written(&device, &queue);
    let (mapped, answered) = mpsc::channel();
    copy.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result);
        });
    let (done, told_done) = mpsc::channel();
    queue.on_submitted_work_done(move || {
        let _ = done.send(());
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the device did not poll: {error}"))?;
    // Not `recv`: the callbacks have run by now, or `poll` waited on nothing.
    let answer = answered
        .try_recv()
        .map_err(|_| "the mapping was not answered before poll returned")?;
    answer.map_err(|error| format!("the buffer did not map: {error}"))?;
    println!("polled: {:?}", read(&copy)?);
    match told_done.try_recv() {
        Ok(()) => println!("submitted work done: told before poll returned"),
        Err(_) => println!("submitted work done: not told before poll returned"),
    }

    let copy = copy_of_written(&device, &queue);
    let (mapped, answered) = oneshot::channel();
    copy.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result);
        });
    answered
        .await
        .map_err(|_| "the mapping was never answered")?
        .map_err(|error| format!("the buffer did not map: {error}"))?;
    println!("awaited: {:?}", read(&copy)?);
    Ok(())
}

async fn device() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no WebGPU adapter: {error}"))?;
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|error| format!("no WebGPU device: {error}"))
}

/// A buffer the GPU copied [`WRITTEN`] into, its copy submitted and not yet waited on.
fn copy_of_written(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Buffer {
    let size = WRITTEN.len() as u64;
    let written = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("written"),
        size,
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let copy = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("copy"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&written, 0, &WRITTEN);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(&written, 0, &copy, 0, size);
    queue.submit([encoder.finish()]);
    copy
}

/// What the mapped `buffer` holds.
fn read(buffer: &wgpu::Buffer) -> Result<Vec<u8>, String> {
    let bytes = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|error| format!("the mapping cannot be read: {error}"))?;
    Ok(bytes.to_vec())
}
