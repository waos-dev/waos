//! Draws what an egui pass left, over the scene, with egui's own wgpu renderer.

use crate::egui_view::Painting;
use crate::gpu::Gpu;

pub struct PanelPainter {
    renderer: egui_wgpu::Renderer,
}

impl PanelPainter {
    /// A painter onto targets of `format`, one sample to a pixel.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> PanelPainter {
        let options = egui_wgpu::RendererOptions::default();
        PanelPainter {
            renderer: egui_wgpu::Renderer::new(device, format, options),
        }
    }

    /// Draws `painting` onto `target`, `size` physical pixels, over what is there, and takes in
    /// the textures the pass made, changed and let go of.
    pub fn paint(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        mut painting: Painting,
    ) {
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: painting.pixels_per_point,
        };
        for (id, deltas) in painting.textures.set.drain() {
            for delta in &deltas {
                self.renderer
                    .update_texture(&gpu.device, &gpu.queue, id, delta);
            }
        }
        self.renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            encoder,
            &painting.primitives,
            &screen,
        );
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("panel"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        self.renderer
            .render(&mut pass, &painting.primitives, &screen);
        drop(pass);
        for id in painting.textures.free.drain() {
            self.renderer.free_texture(&id);
        }
    }
}
