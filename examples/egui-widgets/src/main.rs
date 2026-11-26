//! The smallest program whose UI is egui: a button that counts its clicks and a field that takes
//! text. A view's events reach egui through `wasdk-adapter-egui`'s `State`, as a winit window's
//! do through `egui-winit`'s, and `egui-wgpu` draws what egui laid out on the program's own
//! device.
//!
//! It says what a test drives it by: `window: open`, then where its button and its field are, in
//! the view's pixels, each time one moves (`button at 40,26`), `clicked 1` at each click and
//! `text: "milk"` at each change of the field.
//!
//! ```text
//! cargo build -p egui-widgets --target wasm32-wasip3
//! ```

use std::sync::Arc;

use wasdk::views::{self, View, ViewChange, ViewEvent};
use wasdk::windows::WindowConfig;
use wasdk_adapter_egui::State;

#[wasdk::main]
async fn main() -> Result<(), String> {
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

    let view = Arc::new(first_view().await?);
    let (surface, mut config) = configured_surface(&instance, &adapter, &device, &view)?;
    let egui_ctx = egui::Context::default();
    let mut egui_state = State::new(egui_ctx.clone(), view.clone());
    egui_state.set_max_texture_side(device.limits().max_texture_dimension_2d as usize);
    let options = egui_wgpu::RendererOptions::default();
    let mut renderer = egui_wgpu::Renderer::new(&device, config.format, options);
    let mut widgets = Widgets::default();
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    println!("window: open");
    view.request_frame();
    while let Some(event) = events.next().await {
        let response = egui_state.on_view_event(event);
        if response.repaint {
            view.request_frame();
        }
        match response.event {
            Some(ViewEvent::Frame(_)) => {
                let raw_input = egui_state.take_egui_input();
                let full_output = egui_ctx.run_ui(raw_input, |ui| widgets.show(ui));
                egui_state.handle_platform_output(full_output.platform_output);
                let pixels_per_point = full_output.pixels_per_point;
                let primitives = egui_ctx.tessellate(full_output.shapes, pixels_per_point);
                // The textures are taken in whether or not there is a frame to draw into: egui
                // refuses to have them dropped unapplied.
                let mut textures = full_output.textures_delta;
                for (id, deltas) in textures.set.drain() {
                    for delta in &deltas {
                        renderer.update_texture(&device, &queue, id, delta);
                    }
                }
                if let Some(frame) = frame_of(&surface) {
                    let painting = Painting {
                        primitives: &primitives,
                        screen: egui_wgpu::ScreenDescriptor {
                            size_in_pixels: [config.width, config.height],
                            pixels_per_point,
                        },
                    };
                    paint(&device, &queue, &mut renderer, &frame, &painting);
                    queue.present(frame);
                }
                for id in textures.free.drain() {
                    renderer.free_texture(&id);
                }
                if wants_next_frame_now(&full_output.viewport_output) {
                    view.request_frame();
                }
            }
            Some(ViewEvent::Metrics(metrics)) => {
                config.width = metrics.width;
                config.height = metrics.height;
                surface.configure(&device, &config);
                view.request_frame();
            }
            // No field of egui's has the keyboard, and the program has no keys of its own.
            Some(ViewEvent::Key(key)) => key.reply.handled(false),
            Some(ViewEvent::CloseRequested) => match view.window() {
                Some(window) => window.close(),
                None => return Ok(()),
            },
            _ => {}
        }
    }
    Ok(())
}

/// What the window shows, and what it said of it last.
#[derive(Default)]
struct Widgets {
    clicks: u32,
    text: String,
    said: Option<(Place, Place)>,
}

/// The middle of a widget, in the view's pixels.
#[derive(Clone, Copy, PartialEq)]
struct Place {
    x: i32,
    y: i32,
}

impl Widgets {
    fn show(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let button = ui.button("Count");
            if button.clicked() {
                self.clicks += 1;
                println!("clicked {}", self.clicks);
            }
            ui.label(format!("Clicked {} times", self.clicks));
            let field = ui.text_edit_singleline(&mut self.text);
            if field.changed() {
                println!("text: {:?}", self.text);
            }
            let scale = ui.ctx().pixels_per_point();
            self.say_where(place_of(button.rect, scale), place_of(field.rect, scale));
        });
    }

    /// Says where the button and the field are, once for each place they come to.
    fn say_where(&mut self, button: Place, field: Place) {
        if self.said == Some((button, field)) {
            return;
        }
        self.said = Some((button, field));
        println!("button at {},{}", button.x, button.y);
        println!("field at {},{}", field.x, field.y);
    }
}

fn place_of(rect: egui::Rect, pixels_per_point: f32) -> Place {
    let middle = rect.center() * pixels_per_point;
    Place {
        x: middle.x.round() as i32,
        y: middle.y.round() as i32,
    }
}

/// Whether egui asked for its next pass at once, as it does while something animates.
fn wants_next_frame_now(viewports: &egui::OrderedViewportIdMap<egui::ViewportOutput>) -> bool {
    viewports
        .get(&egui::ViewportId::ROOT)
        .is_some_and(|root| root.repaint_delay.is_zero())
}

/// What an egui pass left to draw.
struct Painting<'a> {
    primitives: &'a [egui::ClippedPrimitive],
    screen: egui_wgpu::ScreenDescriptor,
}

/// Draws `painting` into `frame` with egui's own wgpu renderer, over a clear background.
fn paint(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut egui_wgpu::Renderer,
    frame: &wgpu::SurfaceTexture,
    painting: &Painting<'_>,
) {
    let target = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    renderer.update_buffers(
        device,
        queue,
        &mut encoder,
        painting.primitives,
        &painting.screen,
    );
    let mut pass = encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
        .forget_lifetime();
    renderer.render(&mut pass, painting.primitives, &painting.screen);
    drop(pass);
    queue.submit([encoder.finish()]);
}

/// A surface on `view`, configured for `device` at the view's size in the surface's preferred
/// format, and its configuration.
fn configured_surface(
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    view: &Arc<View>,
) -> Result<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration), String> {
    let surface = instance
        .create_surface(view.clone())
        .map_err(|error| format!("no surface on the view: {error}"))?;
    let metrics = view.metrics();
    let config = surface
        .get_default_config(adapter, metrics.width, metrics.height)
        .ok_or("the view's surface draws in no format")?;
    surface.configure(device, &config);
    Ok((surface, config))
}

/// The frame `surface` hands out to draw into; none when it has none to give.
fn frame_of(surface: &wgpu::Surface<'_>) -> Option<wgpu::SurfaceTexture> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
        _ => None,
    }
}

/// The view that arrives for the program's request.
async fn first_view() -> Result<View, String> {
    // The stream is opened before the request, so the view that arrives for it is not missed.
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("egui widgets");
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
