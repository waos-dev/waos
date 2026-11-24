//! Gives the GPU a shader as naga's module (`ShaderSource::Naga`), the form a program that builds
//! or translates its shaders in memory has, and prints what the shader computed: each number
//! doubled. WaOS's boundary carries WGSL text alone, so the module is written as WGSL on its way.

use std::borrow::Cow;
use std::sync::mpsc;

/// Doubles each number of the buffer bound to it.
const DOUBLES: &str = "
@group(0) @binding(0) var<storage, read_write> numbers: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    numbers[id.x] = numbers[id.x] * 2u;
}
";

const NUMBERS: [u32; 4] = [1, 2, 3, 4];

#[wasdk::main]
async fn main() -> Result<(), String> {
    let (device, queue) = device().await?;
    let module = naga::front::wgsl::parse_str(DOUBLES)
        .map_err(|error| format!("the shader does not parse: {error}"))?;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("doubles"),
        source: wgpu::ShaderSource::Naga(Cow::Owned(module)),
    });
    let messages = shader.get_compilation_info().await.messages;
    println!("compilation messages: {}", messages.len());

    let doubled = doubled(&device, &queue, &shader)?;
    println!("doubled: {doubled:?}");
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

/// [`NUMBERS`] after `shader` ran over them on the GPU, read back.
fn doubled(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shader: &wgpu::ShaderModule,
) -> Result<Vec<u32>, String> {
    let size = size_of_val(&NUMBERS) as u64;
    let numbers = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("numbers"),
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("read"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bytes: Vec<u8> = NUMBERS
        .iter()
        .flat_map(|number| number.to_le_bytes())
        .collect();
    queue.write_buffer(&numbers, 0, &bytes);

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("doubles"),
        layout: None,
        module: shader,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let bound = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("numbers"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: numbers.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bound, &[]);
        pass.dispatch_workgroups(NUMBERS.len() as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&numbers, 0, &read, 0, size);
    queue.submit([encoder.finish()]);

    let (mapped, answered) = mpsc::channel();
    read.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the device did not poll: {error}"))?;
    answered
        .try_recv()
        .map_err(|_| "the mapping was not answered before poll returned")?
        .map_err(|error| format!("the buffer did not map: {error}"))?;
    let bytes = read
        .slice(..)
        .get_mapped_range()
        .map_err(|error| format!("the mapping cannot be read: {error}"))?;
    let (numbers, _) = bytes.as_chunks::<4>();
    Ok(numbers.iter().copied().map(u32::from_le_bytes).collect())
}
