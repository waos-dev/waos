//! The smallest program that draws: one view, one pipeline, a triangle of one colour over the
//! clear colour every frame, drawn the way any wgpu program draws one, on a surface made for its
//! view. The clear colour has no alpha, which the surface's default configuration, opaque, shows
//! as opaque all the same. The program draws on the view it is given, or asks for one when given none; asks for its
//! next frame after each one it presents, so it draws at the screen's rate; closes its window when
//! asked; and ends when its view is gone. The pipeline is made once.
//!
//! ```text
//! cargo build -p triangle --target wasm32-wasip3
//! ```

use std::sync::Arc;

use wasdk::views::{self, View, ViewChange, ViewEvent};
use wasdk::windows::WindowConfig;

/// What the triangle is drawn over, with no alpha: the surface is opaque.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.06,
    g: 0.06,
    b: 0.08,
    a: 0.0,
};

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
    let (surface, config) = configured_surface(&instance, &adapter, &device, &view)?;
    let pipeline = build_pipeline(&device, config.format);
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    view.request_frame();
    while let Some(event) = events.next().await {
        match event {
            ViewEvent::Frame(_) => {
                if let Some(frame) = frame_of(&surface) {
                    let target = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    draw(&device, &queue, &pipeline, &target);
                    queue.present(frame);
                }
                view.request_frame();
            }
            ViewEvent::Metrics(metrics) => {
                let resized = wgpu::SurfaceConfiguration {
                    width: metrics.width,
                    height: metrics.height,
                    ..config.clone()
                };
                surface.configure(&device, &resized);
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
    config.set_title("Triangle");
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

fn build_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("triangle"),
        source: wgpu::ShaderSource::Wgsl(include_str!("triangle.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("triangle"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    target: &wgpu::TextureView,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
        pass.set_pipeline(pipeline);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
}
