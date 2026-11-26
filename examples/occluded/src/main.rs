//! A program that says what its view's surface answers as its view goes in and out of sight: it
//! asks for a view, configures the view's surface, and at its start and each change of its
//! lifecycle state takes the surface's texture and prints the state and the answer, one line
//! each, as `state: answer`. It presents nothing; a texture taken is dropped, as a frame not
//! drawn is.
//!
//! ```text
//! cargo build -p occluded --target wasm32-wasip3
//! ```

use std::sync::Arc;

use wasdk::lifecycle::{self, LifecycleState};
use wasdk::views::{self, View, ViewChange};
use wasdk::windows::WindowConfig;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let mut states = lifecycle::state_changes();
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no adapter: {error}"))?;
    let (device, _queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|error| format!("no device: {error}"))?;
    let view = Arc::new(first_view().await?);
    let surface = instance
        .create_surface(view.clone())
        .map_err(|error| format!("no surface on the view: {error}"))?;
    let metrics = view.metrics();
    let config = surface
        .get_default_config(&adapter, metrics.width, metrics.height)
        .ok_or("the view's surface draws in no format")?;
    surface.configure(&device, &config);
    println!("started: {}", answer(&surface));
    while let Some(state) = states.next().await {
        println!("{}: {}", name(state), answer(&surface));
    }
    Ok(())
}

/// The view that arrives for the program's request.
async fn first_view() -> Result<View, String> {
    let mut changes = views::view_changes();
    views::request_view(&WindowConfig::new(), None).map_err(|error| format!("no view: {error}"))?;
    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(view) => return Ok(view),
            ViewChange::RequestFailed(failure) => return Err(failure.error.to_string()),
            ViewChange::Removed(_) => {}
        }
    }
    Err("the views ended before the view arrived".into())
}

/// What `surface` answers when asked for its texture.
fn answer(surface: &wgpu::Surface<'_>) -> &'static str {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(_) => "a frame",
        wgpu::CurrentSurfaceTexture::Suboptimal(_) => "a suboptimal frame",
        wgpu::CurrentSurfaceTexture::Timeout => "timeout",
        wgpu::CurrentSurfaceTexture::Occluded => "occluded",
        wgpu::CurrentSurfaceTexture::Outdated => "outdated",
        wgpu::CurrentSurfaceTexture::Lost => "lost",
        wgpu::CurrentSurfaceTexture::Validation => "validation",
    }
}

fn name(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::Resumed => "resumed",
        LifecycleState::Inactive => "inactive",
        LifecycleState::Hidden => "hidden",
        LifecycleState::Paused => "paused",
    }
}
