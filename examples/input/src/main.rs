//! A program that says what input its window hears: it asks for a window titled `Input`, draws it
//! in one colour, and says each pointer event, key and change of its text it hears, one line each,
//! as `what: details`. Each press, each scroll and each change of its text draws the window in the
//! next of four colours; a key draws nothing new, and is left unhandled. Its first press starts
//! editing its text, empty, so an input method's text reaches it as the change it makes. Asking to
//! close its window closes it, and the program ends with its view.
//!
//! ```text
//! cargo build -p input --target wasm32-wasip3
//! ```

use wasdk::views::{
    self, KeyInput, PointerChange, PointerEvent, PointerSignalKind, TextAffinity,
    TextCapitalization, TextEditingValue, TextInput, TextInputAction, TextInputConfiguration,
    TextInputType, View, ViewChange, ViewEvent,
};
use wasdk::windows::WindowConfig;
use wasdk_wgpu::SharedView;

/// The colours the window is drawn in, in turn: a dark blue, an orange, a green and a purple.
const COLOURS: [wgpu::Color; 4] = [
    wgpu::Color {
        r: 0.1,
        g: 0.1,
        b: 0.4,
        a: 1.0,
    },
    wgpu::Color {
        r: 0.9,
        g: 0.5,
        b: 0.1,
        a: 1.0,
    },
    wgpu::Color {
        r: 0.1,
        g: 0.7,
        b: 0.3,
        a: 1.0,
    },
    wgpu::Color {
        r: 0.5,
        g: 0.2,
        b: 0.7,
        a: 1.0,
    },
];

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

    let view = SharedView::new(open_window().await?);
    let (surface, config) = configured_surface(&instance, &adapter, &device, &view)?;
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    let mut changes = 0;
    let mut text_input = None;
    view.request_frame();
    while let Some(event) = events.next().await {
        match event {
            ViewEvent::Frame(_) => draw(&surface, &device, &queue, COLOURS[changes % 4]),
            ViewEvent::Metrics(metrics) => {
                let resized = wgpu::SurfaceConfiguration {
                    width: metrics.width,
                    height: metrics.height,
                    ..config.clone()
                };
                surface.configure(&device, &resized);
                view.request_frame();
            }
            ViewEvent::Pointer(pointer) => {
                say_pointer(&pointer);
                if pointer.change == PointerChange::Down && text_input.is_none() {
                    text_input = start_editing(&view);
                }
                if pointer.change == PointerChange::Down
                    || pointer.signal == PointerSignalKind::Scroll
                {
                    changes += 1;
                    view.request_frame();
                }
            }
            ViewEvent::Key(key) => say_key(key),
            ViewEvent::TextInputEditingValue(value) => {
                say_text(&value);
                changes += 1;
                view.request_frame();
            }
            ViewEvent::CloseRequested => {
                if let Some(window) = view.window() {
                    window.close();
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Asks for the window, and waits for it.
async fn open_window() -> Result<View, String> {
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("Input");
    views::request_view(&config, None).map_err(|error| format!("no window: {error}"))?;
    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(view) => {
                println!("window: open");
                return Ok(view);
            }
            ViewChange::RequestFailed(failure) => return Err(failure.error.to_string()),
            ViewChange::Removed(_) => {}
        }
    }
    Err("the views ended before the window arrived".into())
}

/// A surface on `view`, configured for `device` at the view's size, and its configuration.
fn configured_surface(
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    view: &SharedView,
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

/// Draws the window in `colour`, and presents it.
fn draw(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    colour: wgpu::Color,
) {
    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        _ => return,
    };
    let target = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("input"),
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
    queue.submit([encoder.finish()]);
    queue.present(frame);
}

/// Starts editing the window's text, empty; none where the view has no text input.
fn start_editing(view: &View) -> Option<TextInput> {
    let Some(text_input) = view.text_input() else {
        println!("text: no text input");
        return None;
    };
    text_input.set_client(TextInputConfiguration {
        input_type: TextInputType::Text,
        input_action: TextInputAction::Done,
        obscure_text: false,
        autocorrect: false,
        enable_suggestions: false,
        text_capitalization: TextCapitalization::None,
        read_only: false,
    });
    text_input.set_editing_state(&TextEditingValue {
        text: String::new(),
        selection: Some(views::TextSelection {
            base_offset: 0,
            extent_offset: 0,
            affinity: TextAffinity::Downstream,
            is_directional: false,
        }),
        composing: None,
    });
    text_input.show();
    println!("text: editing");
    Some(text_input)
}

fn say_pointer(pointer: &PointerEvent) {
    println!(
        "pointer: {:?} {:?} {:?} device {} at {},{} buttons {} pressure {} scroll {},{}",
        pointer.change,
        pointer.kind,
        pointer.signal,
        pointer.device,
        pointer.x,
        pointer.y,
        pointer.buttons.bits(),
        pointer.pressure,
        pointer.scroll_delta_x,
        pointer.scroll_delta_y,
    );
}

/// Says the key, and leaves it unhandled.
fn say_key(key: KeyInput) {
    let event = &key.event;
    println!(
        "key: {:?} physical {:#x} logical {:#x} character {:?}",
        event.kind, event.physical, event.logical, event.character
    );
    key.reply.handled(false);
}

fn say_text(value: &TextEditingValue) {
    let caret = value
        .selection
        .map(|selection| format!("{}..{}", selection.base_offset, selection.extent_offset));
    println!("text: {:?} selection {caret:?}", value.text);
}
