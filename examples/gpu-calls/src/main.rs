//! Calls `wasdk:gpu` itself, with no wgpu over it, and prints what each call answered: the
//! adapter given, first among those offered; a device opened with a limit set by its setter,
//! refused a feature and a limit the adapter lacks; a buffer written mapped at creation, and one
//! written by the queue and doubled by a compute pass, each read back mapped; a render pass's
//! colour read back; an error caught by an error scope; an error no scope caught, from the
//! device's uncaptured errors; and the device lost once destroyed. What the engine's tests check of its GPU host.
//!
//! ```text
//! cargo build -p gpu-calls --target wasm32-wasip3
//! ```

use wasdk::gpu::{
    Adapter, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferBinding,
    ColorTargetState, ComputePassDescriptor, ComputePipelineDescriptor, Device, DeviceDescriptor,
    FragmentState, Queue, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, RequestAdapterOptions, TexelCopyBufferInfo, TexelCopyTextureInfo,
    VertexState,
};
use wasdk::gpu_types::{
    BufferDescriptor, BufferUsages, Color, ColorLoadOp, ColorOperations, ColorWrites,
    DeviceLostReason, Error, ErrorFilter, Extent3d, Feature, FrontFace, Limits, MapMode,
    MemoryHints, MultisampleState, Origin3d, PipelineCompilationOptions, PolygonMode,
    PowerPreference, PrimitiveState, PrimitiveTopology, ShaderModuleDescriptor, ShaderSource,
    StoreOp, TexelCopyBufferLayout, TextureAspect, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor,
};

/// Doubles each number of the buffer bound to it.
const DOUBLES: &str = "
@group(0) @binding(0) var<storage, read_write> numbers: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    numbers[id.x] = numbers[id.x] * 2u;
}
";

/// Fills its target with red, with one triangle over the whole of it.
const RED: &str = "
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corners = array(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    return vec4(corners[index], 0.0, 1.0);
}

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4(1.0, 0.0, 0.0, 1.0);
}
";

/// Features no adapter the tests run on has, of which the first the adapter lacks is asked for.
const RARE_FEATURES: [Feature; 4] = [
    Feature::ShaderPerVertex,
    Feature::MemoryDecorationVolatile,
    Feature::SelectiveMultiview,
    Feature::TextureCompressionAstcHdr,
];

#[wasdk::main]
async fn main() -> Result<(), String> {
    let adapter = wasdk::gpu::request_adapter(RequestAdapterOptions {
        power_preference: PowerPreference::None,
        force_fallback_adapter: false,
        compatible_surface: None,
    })
    .await
    .ok_or("no adapter")?;
    println!(
        "the adapter given is the first offered: {}",
        first_offered_is(&adapter).await
    );
    refuse_what_the_adapter_lacks(&adapter).await;
    let (device, queue) = open_device(&adapter).await?;
    let mut uncaptured = device
        .uncaptured_errors()
        .ok_or("the first take has the uncaptured errors")?;
    println!(
        "a later take of the uncaptured errors: {}",
        if device.uncaptured_errors().is_none() {
            "none"
        } else {
            "some"
        }
    );

    println!(
        "written at creation: {:?}",
        written_at_creation(&device, &queue)
    );
    println!("doubled: {:?}", doubled(&device, &queue, [1, 2, 3, 4]));
    println!("drawn: {:?}", drawn_pixel(&device, &queue));

    device.push_error_scope(ErrorFilter::Validation);
    invalid_buffer(&device);
    let caught = device.pop_error_scope();
    println!("caught by the scope: {}", kind(caught.as_ref()));

    invalid_buffer(&device);
    let uncaught = uncaptured.next().await;
    println!("uncaptured: {}", kind(uncaught.as_ref()));

    let lost = device.lost();
    device.destroy();
    let info = lost.await;
    println!(
        "lost: {}",
        if info.reason == DeviceLostReason::Destroyed {
            "destroyed"
        } else {
            "unknown"
        }
    );
    Ok(())
}

/// Whether `adapter`, given with no preference, is the first of the adapters offered.
async fn first_offered_is(adapter: &Adapter) -> bool {
    let offered = wasdk::gpu::enumerate_adapters().await;
    let (given, first) = (adapter.info(), offered.first().map(Adapter::info));
    first.is_some_and(|first| (first.name, first.backend) == (given.name, given.backend))
}

/// Asks for a feature and a limit past the adapter's, each refused with wgpu-core's message.
async fn refuse_what_the_adapter_lacks(adapter: &Adapter) {
    let has = adapter.features();
    if let Some(&lacked) = RARE_FEATURES.iter().find(|feature| !has.contains(feature)) {
        let limits = Limits::new();
        let refused = adapter
            .request_device(DeviceDescriptor {
                label: None,
                required_features: vec![lacked],
                required_limits: &limits,
                memory_hints: MemoryHints::Performance,
            })
            .await;
        println!(
            "a feature the adapter lacks: {}",
            refused_or_opened(&refused)
        );
    }
    let limits = adapter.limits();
    limits.set_max_bind_groups(limits.max_bind_groups() + 1);
    let refused = adapter
        .request_device(DeviceDescriptor {
            label: None,
            required_features: Vec::new(),
            required_limits: &limits,
            memory_hints: MemoryHints::Performance,
        })
        .await;
    println!(
        "a limit past the adapter's: {}",
        refused_or_opened(&refused)
    );
}

fn refused_or_opened<T>(answer: &Result<T, String>) -> &'static str {
    match answer {
        Ok(_) => "opened",
        Err(_) => "refused",
    }
}

/// A device with wgpu's default limits but for its bind groups, set to the adapter's own.
async fn open_device(adapter: &Adapter) -> Result<(Device, Queue), String> {
    let limits = Limits::new();
    let most_bind_groups = adapter.limits().max_bind_groups();
    limits.set_max_bind_groups(most_bind_groups);
    let (device, queue) = adapter
        .request_device(DeviceDescriptor {
            label: Some("gpu-calls".to_owned()),
            required_features: Vec::new(),
            required_limits: &limits,
            memory_hints: MemoryHints::MemoryUsage,
        })
        .await?;
    println!(
        "the device has the bind groups asked: {}",
        device.limits().max_bind_groups() == most_bind_groups
    );
    Ok((device, queue))
}

fn buffer(device: &Device, size: u64, usage: BufferUsages, mapped_at_creation: bool) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: None,
        size,
        usage,
        mapped_at_creation,
    })
}

/// The bytes of a buffer of `size` bytes, filled by what `copy` records into it, mapped for
/// reading.
fn read_back(
    device: &Device,
    queue: &Queue,
    size: u64,
    copy: impl FnOnce(&wasdk::gpu::CommandEncoder, &Buffer),
) -> Vec<u8> {
    let readable = buffer(
        device,
        size,
        BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        false,
    );
    let encoder = device.create_command_encoder(None);
    copy(&encoder, &readable);
    queue.submit(vec![wasdk::gpu::CommandEncoder::finish(encoder)]);
    readable
        .map_async(MapMode::Read, 0, None)
        .expect("a buffer made to be read maps");
    let bytes = readable.read_mapped_range(0, size);
    readable.unmap();
    bytes
}

fn numbers(bytes: &[u8]) -> Vec<u32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|word| u32::from_le_bytes(*word)).collect()
}

/// Four numbers written into a buffer mapped at creation, unmapped, and read back.
fn written_at_creation(device: &Device, queue: &Queue) -> Vec<u32> {
    let written = buffer(device, 16, BufferUsages::COPY_SRC, true);
    let bytes: Vec<u8> = [5u32, 6, 7, 8]
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .collect();
    written.write_mapped_range(0, &bytes);
    written.unmap();
    numbers(&read_back(device, queue, 16, |encoder, readable| {
        encoder.copy_buffer_to_buffer(&written, 0, readable, 0, None);
    }))
}

/// `values` written by the queue, doubled by a compute pass, and read back.
fn doubled(device: &Device, queue: &Queue, values: [u32; 4]) -> Vec<u32> {
    let storage = buffer(
        device,
        16,
        BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        false,
    );
    let bytes: Vec<u8> = values.iter().flat_map(|n| n.to_le_bytes()).collect();
    queue.write_buffer(&storage, 0, &bytes);
    let module = device.create_shader_module(&ShaderModuleDescriptor {
        label: Some("doubles".to_owned()),
        source: ShaderSource::Wgsl(DOUBLES.to_owned()),
    });
    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main".to_owned()),
        compilation_options: PipelineCompilationOptions {
            constants: Vec::new(),
        },
    });
    let layout = pipeline.bind_group_layout(0);
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: vec![BindGroupEntry {
            binding: 0,
            resource: BindingResource::Buffer(BufferBinding {
                buffer: &storage,
                offset: 0,
                size: None,
            }),
        }],
    });
    numbers(&read_back(device, queue, 16, |encoder, readable| {
        let pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, Some(&bind_group), &[]);
        pass.dispatch_workgroups(4, 1, 1);
        drop(pass);
        encoder.copy_buffer_to_buffer(&storage, 0, readable, 0, None);
    }))
}

/// The first pixel of a 4×4 texture cleared blue, then drawn over in red.
fn drawn_pixel(device: &Device, queue: &Queue) -> Vec<u8> {
    let size = Extent3d {
        width: 4,
        height: 4,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: Vec::new(),
    });
    let view = texture.create_view(&TextureViewDescriptor {
        label: None,
        format: None,
        dimension: None,
        usage: None,
        aspect: TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    });
    let module = device.create_shader_module(&ShaderModuleDescriptor {
        label: Some("red".to_owned()),
        source: ShaderSource::Wgsl(RED.to_owned()),
    });
    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: VertexState {
            module: &module,
            entry_point: Some("vertex".to_owned()),
            compilation_options: PipelineCompilationOptions {
                constants: Vec::new(),
            },
            buffers: Vec::new(),
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: MultisampleState {
            count: 1,
            mask: u64::MAX,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(FragmentState {
            module: &module,
            entry_point: Some("fragment".to_owned()),
            compilation_options: PipelineCompilationOptions {
                constants: Vec::new(),
            },
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::all(),
            })],
        }),
        multiview_mask: None,
    });
    let bytes = read_back(device, queue, 256 * 4, |encoder, readable| {
        let pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: vec![Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: ColorOperations {
                    load: ColorLoadOp::Clear(Color {
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.draw(
            wasdk::gpu_types::Range { start: 0, end: 3 },
            wasdk::gpu_types::Range { start: 0, end: 1 },
        );
        drop(pass);
        encoder.copy_texture_to_buffer(
            &TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: 0 },
                aspect: TextureAspect::All,
            },
            &TexelCopyBufferInfo {
                buffer: readable,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            size,
        );
    });
    bytes[..4].to_vec()
}

/// A buffer both mapped for reading and for writing, which wgpu-core refuses as invalid.
fn invalid_buffer(device: &Device) {
    buffer(
        device,
        4,
        BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
        false,
    );
}

fn kind(error: Option<&Error>) -> &'static str {
    match error {
        None => "none",
        Some(Error::OutOfMemory) => "out of memory",
        Some(Error::Validation(_)) => "validation",
        Some(Error::Internal(_)) => "internal",
    }
}
