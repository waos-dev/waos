//! Model Viewer: a glTF viewer written without Waft, to show that a WaOS program can bring any
//! framework that draws with wgpu. The scene is drawn with wgpu, the panel with egui, and the
//! view's events reach egui through the app's own glue (`egui_view`).

mod app;
mod camera;
mod egui_view;
mod events;
mod gpu;
mod model;
mod open;
mod render;
mod settings;
mod shown;
mod ui;
mod window;

use wasdk_wgpu::SharedView;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let gpu = gpu::Gpu::request().await?;
    let view = SharedView::new(window::open().await?);
    app::App::new(gpu, view)?.run().await
}
