//! Reads a buffer back from the GPU as native wgpu programs do, and prints what it read.
//!
//! The program asks for a mapping, calls `Device::poll` with `Wait`, and reads the answer at
//! once: the callback has run before `poll` returns, as on native wgpu, and so has a queue's
//! submitted-work callback asked after it, in the order wgpu promises. What is read back was
//! written through a buffer made mapped, as `create_buffer_init` writes a buffer's first data.
//!
//! Each request reaches the GPU at the call, as in wgpu: submitted work is told done once the work
//! submitted before the ask is, whatever is submitted after; an `unmap` before the mapping is
//! answered aborts it and leaves the buffer unmapped; and `poll` with `Poll` runs the callbacks
//! whose work is done, as does the instance's `poll_all`, which polls every device the program
//! made, as native wgpu's does. A buffer mapped for writing, which the GPU never writes, keeps
//! what was written in it between mappings, and a mapping written in part changes only that part.

use std::sync::mpsc;

use wgpu::util::DeviceExt;

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

    println!("{}", done_before_a_later_submission(&device, &queue)?);
    println!("{}", unmapped_before_mapped(&device, &queue)?);
    println!("{}", polled_without_waiting(&device, &queue)?);
    println!("{}", polled_through_the_instance(&device, &queue));
    println!("written in part: {:?}", written_in_part(&device, &queue)?);
    Ok(())
}

/// What a buffer mapped for writing holds after a first mapping writes [`WRITTEN`] in two ranges
/// and a second writes its first four bytes again, each read back through a copy.
fn written_in_part(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Vec<u8>, String> {
    let size = WRITTEN.len() as u64;
    let written = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("written"),
        size,
        usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    write_mapped(device, &written, &[(0, &WRITTEN[..4]), (4, &WRITTEN[4..])])?;
    write_mapped(device, &written, &[(0, &[9; 4])])?;
    let copy = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("copy"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(&written, 0, &copy, 0, size);
    queue.submit([encoder.finish()]);
    copy.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the device did not poll: {error}"))?;
    read(&copy)
}

/// Maps `buffer` for writing, writes each of `ranges` at its offset through a range of its own,
/// and unmaps it.
fn write_mapped(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    ranges: &[(u64, &[u8])],
) -> Result<(), String> {
    buffer.slice(..).map_async(wgpu::MapMode::Write, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the device did not poll: {error}"))?;
    for &(offset, bytes) in ranges {
        let end = offset + bytes.len() as u64;
        let mut range = buffer
            .slice(offset..end)
            .get_mapped_range_mut()
            .map_err(|error| format!("the mapping cannot be written: {error}"))?;
        range.copy_from_slice(bytes);
    }
    buffer.unmap();
    Ok(())
}

/// Submitted work asked about between two submissions is told done once the first is.
fn done_before_a_later_submission(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<&'static str, String> {
    let earlier = submit_copy(device, queue);
    let (done, told_done) = mpsc::channel();
    queue.on_submitted_work_done(move || {
        let _ = done.send(());
    });
    submit_copy(device, queue);
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(earlier),
            timeout: None,
        })
        .map_err(|error| format!("the device did not poll: {error}"))?;
    Ok(match told_done.try_recv() {
        Ok(()) => "work done before a later submission: told once the earlier was",
        Err(_) => "work done before a later submission: not told",
    })
}

/// A mapping unmapped before it is answered is aborted, and the buffer maps again.
fn unmapped_before_mapped(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<&'static str, String> {
    let copy = copy_of_written(device, queue);
    let (mapped, answered) = mpsc::channel();
    copy.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result.is_ok());
        });
    copy.unmap();
    let aborted = matches!(answered.try_recv(), Ok(false));
    let (mapped, answered) = mpsc::channel();
    copy.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result.is_ok());
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the device did not poll: {error}"))?;
    let maps_again = matches!(answered.try_recv(), Ok(true)) && read(&copy)? == WRITTEN;
    Ok(match (aborted, maps_again) {
        (true, true) => "unmapped before mapped: aborted, and maps again",
        (true, false) => "unmapped before mapped: aborted, and does not map again",
        (false, _) => "unmapped before mapped: not aborted",
    })
}

/// A mapping whose work is done is answered by a poll that does not wait.
fn polled_without_waiting(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<&'static str, String> {
    let copy = copy_of_written(device, queue);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the device did not poll: {error}"))?;
    let (mapped, answered) = mpsc::channel();
    copy.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result.is_ok());
        });
    device
        .poll(wgpu::PollType::Poll)
        .map_err(|error| format!("the device did not poll: {error}"))?;
    Ok(match answered.try_recv() {
        Ok(true) => "polled without waiting: answered",
        Ok(false) => "polled without waiting: refused",
        Err(_) => "polled without waiting: not answered",
    })
}

/// A mapping is answered by the instance's `poll_all`, which polls every device.
fn polled_through_the_instance(device: &wgpu::Device, queue: &wgpu::Queue) -> &'static str {
    let copy = copy_of_written(device, queue);
    let (mapped, answered) = mpsc::channel();
    copy.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result.is_ok());
        });
    wasdk_wgpu::instance().poll_all(true);
    match answered.try_recv() {
        Ok(true) => "polled through the instance: answered",
        Ok(false) => "polled through the instance: refused",
        Err(_) => "polled through the instance: not answered",
    }
}

/// Submits a copy of a buffer into another, answering its submission.
fn submit_copy(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::SubmissionIndex {
    let size = WRITTEN.len() as u64;
    let [source, destination] = ["source", "destination"].map(|label| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(&source, 0, &destination, 0, size);
    queue.submit([encoder.finish()])
}

async fn device() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no WebGPU adapter: {error}"))?;
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            // wgpu's defaults within what the adapter has: some fall below them, as the
            // iOS simulator's does.
            required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
            ..Default::default()
        })
        .await
        .map_err(|error| format!("no WebGPU device: {error}"))
}

/// A buffer the GPU copied [`WRITTEN`] into, its copy submitted and not yet waited on. What is
/// copied is written the way a buffer is given its first data: made mapped, written, unmapped.
fn copy_of_written(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Buffer {
    let size = WRITTEN.len() as u64;
    let written = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("written"),
        contents: &WRITTEN,
        usage: wgpu::BufferUsages::COPY_SRC,
    });
    let copy = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("copy"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
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
