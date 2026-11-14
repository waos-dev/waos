//! Draws a frame of the scene: the backdrop, the model's surfaces, a wireframe's lines, the
//! ground, and last the surfaces that blend, the farthest first.

use glam::{Mat4, Vec3};

use super::canvas::Canvas;
use super::globals::{Frame, Globals, Shading};
use super::gpu_model::{DrawCall, GpuModel};
use super::pipelines::{Layouts, Pipelines};
use crate::gpu::Gpu;

pub struct SceneRenderer {
    layouts: Layouts,
    pipelines: Pipelines,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    format: wgpu::TextureFormat,
}

/// The part of the target the scene is drawn in, in physical pixels.
#[derive(Clone, Copy)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Where a frame of the scene goes: drawn in `area` of the canvas's multisampled textures, and
/// resolved onto `resolved`, the frame's own texture.
pub struct Target<'a> {
    pub canvas: &'a Canvas,
    pub resolved: &'a wgpu::TextureView,
    pub area: Area,
}

impl SceneRenderer {
    /// A renderer onto targets of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> SceneRenderer {
        let layouts = Layouts::new(device);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &layouts.globals,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        SceneRenderer {
            pipelines: Pipelines::new(device, format, &layouts),
            layouts,
            globals,
            globals_group,
            format,
        }
    }

    /// The layouts a model's bind groups are made for.
    pub fn layouts(&self) -> &Layouts {
        &self.layouts
    }

    /// Draws `model`, posed by `world`, onto `target` as `frame` says.
    pub fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target<'_>,
        frame: &Frame,
        model: &GpuModel,
        world: &[Mat4],
    ) {
        let globals = Globals::new(frame, self.format);
        gpu.queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let mut pass = begin_pass(encoder, target);
        let area = target.area;
        if area.width == 0 || area.height == 0 {
            return;
        }
        pass.set_viewport(
            area.x as f32,
            area.y as f32,
            area.width as f32,
            area.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(area.x, area.y, area.width, area.height);
        pass.set_bind_group(0, &self.globals_group, &[]);
        pass.set_pipeline(&self.pipelines.backdrop);
        pass.draw(0..3, 0..1);

        let (blended, solid): (Vec<_>, Vec<_>) = model
            .draw_calls()
            .zip(model.draw_nodes())
            .filter(|(call, _)| call.triangle_index_count > 0)
            .partition(|(call, _)| call.kind.blended);
        for (call, _) in &solid {
            self.draw_surfaces(&mut pass, model, call);
        }
        if frame.shading == Shading::Wireframe {
            pass.set_pipeline(&self.pipelines.lines);
            for (call, _) in solid.iter().chain(&blended) {
                draw_edges(&mut pass, model, call);
            }
        }
        if frame.ground.opacity > 0.0 {
            pass.set_pipeline(&self.pipelines.ground);
            pass.draw(0..4, 0..1);
        }
        for call in farthest_first(blended, world, frame.camera) {
            self.draw_surfaces(&mut pass, model, &call);
        }
    }

    fn draw_surfaces(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        model: &GpuModel,
        call: &DrawCall<'_>,
    ) {
        pass.set_pipeline(self.pipelines.surfaces(call.kind));
        pass.set_bind_group(1, model.draw_group(), &[call.offset]);
        pass.set_bind_group(2, call.material, &[]);
        pass.set_vertex_buffer(0, call.vertices.slice(..));
        pass.set_index_buffer(call.triangles.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..call.triangle_index_count, 0, 0..1);
    }
}

fn draw_edges(pass: &mut wgpu::RenderPass<'_>, model: &GpuModel, call: &DrawCall<'_>) {
    pass.set_bind_group(1, model.draw_group(), &[call.offset]);
    pass.set_vertex_buffer(0, call.vertices.slice(..));
    pass.set_index_buffer(call.edges.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..call.edge_index_count, 0, 0..1);
}

/// The draws that blend, from the one whose middle is farthest from `camera` to the nearest, so
/// each is blended over what is behind it.
fn farthest_first<'a>(
    mut draws: Vec<(DrawCall<'a>, usize)>,
    world: &[Mat4],
    camera: Vec3,
) -> impl Iterator<Item = DrawCall<'a>> {
    let distance = |(call, node): &(DrawCall<'_>, usize)| {
        world[*node]
            .transform_point3(call.centre)
            .distance_squared(camera)
    };
    draws.sort_by(|one, other| distance(other).total_cmp(&distance(one)));
    draws.into_iter().map(|(call, _)| call)
}

/// A pass that draws into the canvas's multisampled colour and depth, both cleared, and resolves
/// the colour onto the frame's texture.
fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &Target<'_>,
) -> wgpu::RenderPass<'a> {
    let canvas = target.canvas;
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("scene"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: canvas.colour(),
            depth_slice: None,
            resolve_target: Some(target.resolved),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Discard,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: canvas.depth(),
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}
