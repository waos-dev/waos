//! A program that places composition layers beneath its drawing: on the system's compositor it
//! makes an opaque layer, the video, and one that is not, the badge, and says `layers made`; sets
//! its view's layers to both, the video first and so beneath, under every kind of mutator the WIT
//! carries, and presents its first frame, then to the badge alone, moved, which takes the video
//! off with its second frame. Where the system has no compositor it says `no compositor` and
//! presents its frames with no layers.
//! Each frame draws only the clear colour, which a program leaves clear where its layers show;
//! this one, with nothing presented into its layers, shows nothing of them. It asks for no frame
//! after the second, closes its window when asked, and ends when its view is gone.
//!
//! ```text
//! cargo build -p composition-layers --target wasm32-wasip2
//! ```

use wasdk::views::{
    self, CompositionLayer, Compositor, Mutator, PlacedLayer, Placement, RoundedRect, View,
    ViewChange, ViewEvent,
};
use wasdk::windows::{Point, Rect, Size, WindowConfig};
use wasdk_wgpu::SharedView;

/// What each frame draws: nothing at all, so every layer shows through it.
const CLEAR: wgpu::Color = wgpu::Color::TRANSPARENT;

/// How many frames the program presents.
const FRAMES: usize = 2;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no WebGPU adapter: {error}"))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|error| format!("no WebGPU device: {error}"))?;

    let view = SharedView::new(first_view().await?);
    let surface = configured_surface(&instance, &adapter, &device, &view)?;
    let layers = views::get_compositor().map(|compositor| Layers::on(&compositor));
    println!(
        "{}",
        match layers {
            Some(_) => "layers made",
            None => "no compositor",
        }
    );
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    let mut presented = 0;
    view.request_frame();
    while let Some(event) = events.next().await {
        match event {
            ViewEvent::Frame(_) => {
                let Some(frame) = frame_of(&surface) else {
                    view.request_frame();
                    continue;
                };
                clear(&device, &queue, &frame.texture);
                let placed = layers
                    .as_ref()
                    .map_or_else(Vec::new, |layers| layers.of_frame(presented));
                view.set_layers(&placed);
                queue.present(frame);
                presented += 1;
                if presented < FRAMES {
                    view.request_frame();
                }
            }
            ViewEvent::CloseRequested => match view.window() {
                Some(window) => window.close(),
                None => return Ok(()),
            },
            _ => {}
        }
    }
    Ok(())
}

/// The program's two layers.
struct Layers {
    video: CompositionLayer,
    badge: CompositionLayer,
}

impl Layers {
    /// The video, opaque, and the badge, made on `compositor` in that order.
    fn on(compositor: &Compositor) -> Layers {
        Layers {
            video: compositor.make_layer(true),
            badge: compositor.make_layer(false),
        }
    }

    /// The layers the view holds for frame `frame`, the bottom-most first: both in the first, the
    /// badge alone after it.
    fn of_frame(&self, frame: usize) -> Vec<PlacedLayer<'_>> {
        if frame == 0 {
            vec![
                PlacedLayer {
                    layer: &self.video,
                    placement: video_placement(),
                },
                PlacedLayer {
                    layer: &self.badge,
                    placement: Placement {
                        rect: rect(16.0, 16.0, 96.0, 48.0),
                        mutators: Vec::new(),
                    },
                },
            ]
        } else {
            vec![PlacedLayer {
                layer: &self.badge,
                placement: Placement {
                    rect: rect(32.0, 32.0, 96.0, 48.0),
                    mutators: vec![Mutator::Transform(scale(2.0))],
                },
            }]
        }
    }
}

/// The video at 640 by 360 in a page shown at twice its size, cut by the page's rectangle and the
/// card's rounded corners, at half its alpha.
fn video_placement() -> Placement {
    let corner = Size {
        width: 8.0,
        height: 8.0,
    };
    Placement {
        rect: rect(0.0, 0.0, 640.0, 360.0),
        mutators: vec![
            Mutator::Transform(scale(2.0)),
            Mutator::ClipRect(rect(0.0, 0.0, 600.0, 300.0)),
            Mutator::ClipRoundedRect(RoundedRect {
                rect: rect(10.0, 10.0, 300.0, 200.0),
                top_left: corner,
                top_right: corner,
                bottom_right: corner,
                bottom_left: corner,
            }),
            Mutator::Opacity(128),
        ],
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        origin: Point { x, y },
        size: Size { width, height },
    }
}

/// A scale by `factor` on both axes, as a column-major 4×4 matrix.
fn scale(factor: f32) -> Vec<f32> {
    vec![
        factor, 0.0, 0.0, 0.0, //
        0.0, factor, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]
}

/// A surface on `view`, configured for `device` at the view's size in the surface's preferred
/// format.
fn configured_surface(
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    view: &SharedView,
) -> Result<wgpu::Surface<'static>, String> {
    let surface = instance
        .create_surface(view.clone())
        .map_err(|error| format!("no surface on the view: {error}"))?;
    let metrics = view.metrics();
    let config = surface
        .get_default_config(adapter, metrics.width, metrics.height)
        .ok_or("the view's surface draws in no format")?;
    surface.configure(device, &config);
    Ok(surface)
}

/// The frame `surface` hands out to draw into; none when it has none to give.
fn frame_of(surface: &wgpu::Surface<'_>) -> Option<wgpu::SurfaceTexture> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
        _ => None,
    }
}

/// The view the shell gave, or the one that arrives for a request when it gave none.
async fn first_view() -> Result<View, String> {
    // The stream is opened before the list is read, so a view between the two is not missed.
    let mut changes = views::view_changes();
    if let Some(view) = views::views().into_iter().next() {
        return Ok(view);
    }
    let config = WindowConfig::new();
    config.set_title("Composition layers");
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

/// Clears `texture`, the frame's, to [`CLEAR`].
fn clear(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) {
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(CLEAR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    queue.submit([encoder.finish()]);
}
