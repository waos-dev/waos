//! A program that draws into GPU memory of its own and shows it beside its canvas: with the
//! system's hardware buffer allocator it allocates two buffers of 64 by 48 for drawing, sampling
//! and showing, in the format the system says it allocates, the green one for reading back too,
//! saying first what the system refuses (`rgba8: unsupported` on Apple's systems, `bgra8: unsupported` on
//! Android, and a side past the largest texture, `16385x48: too-large`), then
//! `allocated 2 buffers of 64x48`, and what the red one, asked for no reading back, can be used
//! for, as the system answers it: `red can be gpu-sampled, gpu-color-output, composer-overlay,
//! cpu-read` on Apple's systems, which give a buffer every use, and the same without `cpu-read` on
//! Android, which gives a buffer what it was allocated for;
//! through a texture it imports of each on its own device, clears one red with a render pass and writes the other's pixels green with
//! `write_texture`, submitting nothing after, since whatever reads or shows a buffer submits the
//! program's writes first; reads the green one's pixels back, saying
//! `read 64x48 bgra8 premultiplied, first pixel [0, 255, 0, 255]`, the bytes as they are in memory;
//! and encodes the red one with the system's hardware codecs, saying
//! `encoded image/png from 64x48`. Then, in a view of its own, it makes a composition layer on the
//! system's compositor, presents the red buffer on it, saying `presented red`, places the layer in
//! its view at 16, 16, beside its canvas, which it clears grey, writing clear pixels where the
//! layer shows after the clear is submitted, and presents its view; at each frame after, it
//! presents the green buffer, saying `presented green` the first time, until the compositor is
//! done with the red one, which it says as `red is back`, and asks for no frame after. It closes
//! its window when asked, and ends when its view is gone. Where the system allocates no hardware
//! buffers it says `hardware buffer allocator: none`, no hardware codecs
//! `hardware image codecs: none`, and no compositor `no compositor`.
//!
//! ```text
//! cargo build -p hardware-buffers --target wasm32-wasip3
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use wasdk::hardware_buffers::{
    AlphaType, HardwareBuffer, HardwareBufferAllocator, HardwareBufferUsage, ImageError,
    ImageErrorKind, PixelFormat,
};
use wasdk::image_codecs;
use wasdk::views::{
    self, CompositionLayer, Mutator, PlacedLayer, Placement, View, ViewChange, ViewEvent,
};
use wasdk::windows::{Point, Rect, Size, WindowConfig};

/// Each buffer's size, in pixels.
const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

/// What the red buffer is for: drawn into, sampled, and shown by the system's compositor, which on
/// Android samples a buffer it does not show as an overlay.
const SHOWN: HardwareBufferUsage = HardwareBufferUsage::GPU_SAMPLED
    .union(HardwareBufferUsage::GPU_COLOR_OUTPUT)
    .union(HardwareBufferUsage::COMPOSER_OVERLAY);

/// What the green buffer is for: shown, and read back.
const SHOWN_AND_READ: HardwareBufferUsage = SHOWN.union(HardwareBufferUsage::CPU_READ);

/// Where the layer sits in the view, in its pixels: its rectangle's left and top.
const LAYER_AT: [u32; 2] = [16, 16];

/// The green buffer's pixel, as its bytes lie in BGRA and in RGBA alike.
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// The canvas's colour where the layer does not show.
const GREY: wgpu::Color = wgpu::Color {
    r: 0.25,
    g: 0.25,
    b: 0.25,
    a: 1.0,
};

/// How many frames the program waits, presenting the green buffer, for the red one to come back.
const PATIENCE: usize = 600;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let gpu = Gpu::new().await?;
    let Some(allocator) = wasdk_wgpu::hardware_buffer_allocator(&gpu.device) else {
        println!("hardware buffer allocator: none");
        return Ok(());
    };
    let format = system_format(&allocator);
    say_too_large(&allocator, format);
    let red = allocated(&allocator, format, SHOWN)?;
    let green = allocated(&allocator, format, SHOWN_AND_READ)?;
    println!("allocated 2 buffers of {WIDTH}x{HEIGHT}");
    println!("red can be {}", described_usage(red.usage()));
    gpu.clear(
        &wasdk_wgpu::import_texture(&gpu.device, &red),
        wgpu::Color::RED,
    );
    gpu.fill(&wasdk_wgpu::import_texture(&gpu.device, &green), GREEN);
    println!("{}", read_back(&green).await);
    println!("{}", encoded(&red).await);

    let view = Arc::new(first_view().await?);
    let surface = gpu.surface_on(&view)?;
    let Some(compositor) = views::get_compositor() else {
        println!("no compositor");
        gpu.draw_canvas(&surface);
        return close_when_asked(&view).await;
    };
    let layer = compositor.make_layer(true);
    let red_back = Rc::new(Cell::new(false));
    let back = layer.present(&red);
    wasdk::spawn({
        let red_back = Rc::clone(&red_back);
        async move {
            back.await;
            red_back.set(true);
        }
    });
    println!("presented red");
    view.set_layers(&[placed(&layer, view.metrics().device_pixel_ratio)]);
    gpu.draw_canvas(&surface);
    show_green_until_red_is_back(&gpu, &surface, &view, &layer, &green, &red_back).await
}

/// The format the system says it allocates buffers in, saying the one it refuses, which
/// `allocate` refuses too: BGRA on Apple's systems, which refuse RGBA, and RGBA on Android, which
/// refuses BGRA.
fn system_format(allocator: &HardwareBufferAllocator) -> PixelFormat {
    let (format, other) = if allocator.supports(PixelFormat::Bgra8, SHOWN) {
        (PixelFormat::Bgra8, PixelFormat::Rgba8)
    } else {
        (PixelFormat::Rgba8, PixelFormat::Bgra8)
    };
    if !allocator.supports(other, SHOWN)
        && let Err(error) = allocator.allocate(WIDTH, HEIGHT, other, SHOWN)
    {
        println!("{}: {}", name_of(other), kind_of(&error));
    }
    format
}

/// Says that the system refuses a side past the largest texture it makes.
fn say_too_large(allocator: &HardwareBufferAllocator, format: PixelFormat) {
    if let Err(error) = allocator.allocate(16385, HEIGHT, format, SHOWN) {
        println!("16385x{HEIGHT}: {}", kind_of(&error));
    }
}

/// A buffer of [`WIDTH`] by [`HEIGHT`] in `format`, for at least `usage`.
fn allocated(
    allocator: &HardwareBufferAllocator,
    format: PixelFormat,
    usage: HardwareBufferUsage,
) -> Result<HardwareBuffer, String> {
    allocator
        .allocate(WIDTH, HEIGHT, format, usage)
        .map_err(|error| format!("no buffer of {WIDTH}x{HEIGHT}: {}", error.message))
}

/// The flags of `usage`, as the WIT names them, in its order.
fn described_usage(usage: HardwareBufferUsage) -> String {
    let named = [
        (HardwareBufferUsage::GPU_SAMPLED, "gpu-sampled"),
        (HardwareBufferUsage::GPU_COLOR_OUTPUT, "gpu-color-output"),
        (HardwareBufferUsage::COMPOSER_OVERLAY, "composer-overlay"),
        (HardwareBufferUsage::CPU_READ, "cpu-read"),
    ];
    let names: Vec<&str> = named
        .into_iter()
        .filter(|(flag, _)| usage.contains(*flag))
        .map(|(_, name)| name)
        .collect();
    names.join(", ")
}

/// A format's name, as the WIT names it.
fn name_of(format: PixelFormat) -> &'static str {
    match format {
        PixelFormat::Rgba8 => "rgba8",
        PixelFormat::Bgra8 => "bgra8",
    }
}

/// What `buffer`'s pixels read back as: their size, format, alpha and first pixel.
async fn read_back(buffer: &HardwareBuffer) -> String {
    match buffer.read_pixels().await {
        Ok(pixels) => {
            let format = name_of(pixels.format);
            let alpha = match pixels.alpha {
                AlphaType::Premultiplied => "premultiplied",
                AlphaType::Straight => "straight",
            };
            format!(
                "read {}x{} {format} {alpha}, first pixel {:?}",
                pixels.width,
                pixels.height,
                &pixels.bytes[..4]
            )
        }
        Err(error) => format!("read failed: {}", kind_of(&error)),
    }
}

/// What encoding `buffer` as PNG through the system's hardware codecs answers.
async fn encoded(buffer: &HardwareBuffer) -> String {
    let Some(hardware) =
        image_codecs::get_image_codecs().and_then(|codecs| codecs.hardware_image_codecs())
    else {
        return "hardware image codecs: none".to_owned();
    };
    match hardware.encode(buffer, "image/png".to_owned(), None).await {
        Ok(_) => format!(
            "encoded image/png from {}x{}",
            buffer.width(),
            buffer.height()
        ),
        Err(error) => format!("encoding failed: {}", kind_of(&error)),
    }
}

/// At each of `view`'s frames, presents `green` on `layer` and draws the canvas, until the
/// compositor is done with the red buffer, as `red_back` says, then asks for no more; and closes
/// the window when asked.
async fn show_green_until_red_is_back(
    gpu: &Gpu,
    surface: &wgpu::Surface<'static>,
    view: &Arc<View>,
    layer: &CompositionLayer,
    green: &HardwareBuffer,
    red_back: &Cell<bool>,
) -> Result<(), String> {
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    let mut frames = 0;
    view.request_frame();
    while let Some(event) = events.next().await {
        match event {
            ViewEvent::Frame(_) if red_back.get() => println!("red is back"),
            ViewEvent::Frame(_) => {
                // The answer is not waited for: the green buffer is presented again at each frame.
                drop(layer.present(green));
                if frames == 0 {
                    println!("presented green");
                }
                view.set_layers(&[placed(layer, view.metrics().device_pixel_ratio)]);
                gpu.draw_canvas(surface);
                frames += 1;
                if frames < PATIENCE {
                    view.request_frame();
                }
            }
            ViewEvent::CloseRequested => return close(view),
            _ => {}
        }
    }
    Ok(())
}

/// Closes `view`'s window when asked, or ends the program where it has none; ends once its
/// events do.
async fn close_when_asked(view: &View) -> Result<(), String> {
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    while let Some(event) = events.next().await {
        if let ViewEvent::CloseRequested = event {
            return close(view);
        }
    }
    Ok(())
}

fn close(view: &View) -> Result<(), String> {
    if let Some(window) = view.window() {
        window.close();
    }
    Ok(())
}

/// `layer` where it sits beside the canvas of a view of `pixel_ratio` pixels to the logical pixel:
/// [`WIDTH`] by [`HEIGHT`] physical pixels at [`LAYER_AT`]. A layer's buffer is taken as drawn at
/// the view's pixel ratio, so the rectangle is in logical pixels under a transform by the ratio,
/// and the buffer shows one pixel to one.
fn placed(layer: &CompositionLayer, pixel_ratio: f64) -> PlacedLayer<'_> {
    let logical = |pixels: u32| f64::from(pixels) / pixel_ratio;
    PlacedLayer {
        layer,
        placement: Placement {
            rect: Rect {
                origin: Point {
                    x: logical(LAYER_AT[0]),
                    y: logical(LAYER_AT[1]),
                },
                size: Size {
                    width: logical(WIDTH),
                    height: logical(HEIGHT),
                },
            },
            mutators: vec![Mutator::Transform(scale_by(pixel_ratio as f32))],
        },
    }
}

/// The 4×4 matrix, column-major, that scales x and y by `factor`.
fn scale_by(factor: f32) -> Vec<f32> {
    let mut matrix = vec![0.0; 16];
    matrix[0] = factor;
    matrix[5] = factor;
    matrix[10] = 1.0;
    matrix[15] = 1.0;
    matrix
}

/// A failure's kind, as the WIT names it.
fn kind_of(error: &ImageError) -> &'static str {
    match error.kind {
        ImageErrorKind::Unsupported => "unsupported",
        ImageErrorKind::Damaged => "damaged",
        ImageErrorKind::TooLarge => "too-large",
        ImageErrorKind::NotFound => "not-found",
        ImageErrorKind::OutOfMemory => "out-of-memory",
        ImageErrorKind::Other => "other",
    }
}

/// The engine's WebGPU instance, adapter, device and queue.
struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    async fn new() -> Result<Gpu, String> {
        let instance = wasdk_wgpu::instance();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|error| format!("no WebGPU adapter: {error}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| format!("no WebGPU device: {error}"))?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Clears `texture` to `colour`.
    fn clear(&self, texture: &wgpu::Texture, colour: wgpu::Color) {
        let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(colour),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.queue.submit([encoder.finish()]);
    }

    /// Writes `pixel` into every pixel of `texture`, a buffer's, submitting nothing after: what
    /// reads or shows the buffer submits the write first.
    fn fill(&self, texture: &wgpu::Texture, pixel: [u8; 4]) {
        let pixels = pixel.repeat((WIDTH * HEIGHT) as usize);
        self.write_pixels(texture, [0, 0], &pixels);
    }

    /// A surface on `view`, configured at the view's size in the surface's preferred format, for
    /// drawing into and copying into.
    fn surface_on(&self, view: &Arc<View>) -> Result<wgpu::Surface<'static>, String> {
        let surface = self
            .instance
            .create_surface(view.clone())
            .map_err(|error| format!("no surface on the view: {error}"))?;
        let metrics = view.metrics();
        let mut config = surface
            .get_default_config(&self.adapter, metrics.width, metrics.height)
            .ok_or("the view's surface draws in no format")?;
        config.usage |= wgpu::TextureUsages::COPY_DST;
        surface.configure(&self.device, &config);
        Ok(surface)
    }

    /// Draws the canvas grey, then writes it clear where the layer shows, and presents the view,
    /// with the layers it holds beneath its picture, the write submitted as it presents; a view
    /// whose surface hands out no texture draws nothing.
    fn draw_canvas(&self, surface: &wgpu::Surface<'_>) {
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => return,
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(GREY),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.queue.submit([encoder.finish()]);
        let clear = [0; 4].repeat((WIDTH * HEIGHT) as usize);
        self.write_pixels(&frame.texture, LAYER_AT, &clear);
        self.queue.present(frame);
    }

    /// Writes `pixels`, [`HEIGHT`] rows of [`WIDTH`] in the texture's format, into `texture` at
    /// `origin`, its left and top, with the queue's own copy.
    fn write_pixels(&self, texture: &wgpu::Texture, origin: [u32; 2], pixels: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// The view that arrives for the program's request.
async fn first_view() -> Result<View, String> {
    // The stream is opened before the request, so the view that arrives for it is not missed.
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("Hardware buffers");
    views::request_view(&config, None)
        .map_err(|error| format!("no view for this program: {error}"))?;
    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(view) => return Ok(view),
            ViewChange::RequestFailed(failure) => {
                return Err(format!(
                    "request {} failed: {}",
                    failure.request, failure.error
                ));
            }
            ViewChange::Removed(_) => {}
        }
    }
    Err("the views ended before one arrived".into())
}
