//! What every shader of the scene reads, as the GPU holds it: the camera, the light, the
//! backdrop, the ground and how the scene is shown.
//!
//! The invariant: `Globals` is laid out as `shaders/globals.wgsl` declares it, field for field.

use glam::{Mat4, Vec3};

/// How the model's surfaces are drawn.
#[derive(Clone, Copy, PartialEq)]
pub enum Shading {
    /// Each material as it is.
    Shaded,
    /// Every surface in one pale matte colour, with its triangles' edges drawn over it.
    Wireframe,
    /// Each surface's normal as a colour.
    Normals,
}

/// The ground's grid: where it lies and how it is ruled.
#[derive(Clone, Copy)]
pub struct Ground {
    pub centre: Vec3,
    pub cell: f32,
    pub reach: f32,
    /// Its lines' colour, as displayed.
    pub ink: [f32; 3],
    /// Zero hides it.
    pub opacity: f32,
}

/// One frame's settings, in the app's terms.
pub struct Frame {
    pub view_projection: Mat4,
    pub camera: Vec3,
    pub exposure: f32,
    /// The direction towards the light, and its strength: none when the light is off.
    pub light: Option<(Vec3, f32)>,
    /// The backdrop's colours at the top and at the bottom, as displayed.
    pub backdrop: [[f32; 3]; 2],
    pub ground: Ground,
    pub shading: Shading,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Globals {
    view_projection: [[f32; 4]; 4],
    camera: [f32; 4],
    light: [f32; 4],
    backdrop_top: [f32; 4],
    backdrop_bottom: [f32; 4],
    ground: [f32; 4],
    ground_centre: [f32; 4],
    ground_ink: [f32; 4],
    modes: [u32; 4],
}

impl Globals {
    /// `frame` for a target of `format`: one that is not sRGB has the shaders encode colours.
    pub fn new(frame: &Frame, format: wgpu::TextureFormat) -> Globals {
        let (to_light, strength) = frame.light.unwrap_or((Vec3::Y, 0.0));
        let [top, bottom] = frame.backdrop;
        let ground = frame.ground;
        let shading = match frame.shading {
            Shading::Shaded => 0,
            Shading::Wireframe => 1,
            Shading::Normals => 2,
        };
        Globals {
            view_projection: frame.view_projection.to_cols_array_2d(),
            camera: frame.camera.extend(frame.exposure).to_array(),
            light: to_light.extend(strength).to_array(),
            backdrop_top: [top[0], top[1], top[2], 1.0],
            backdrop_bottom: [bottom[0], bottom[1], bottom[2], 1.0],
            ground: [ground.centre.y, ground.cell, ground.reach, ground.opacity],
            ground_centre: ground.centre.extend(1.0).to_array(),
            ground_ink: [ground.ink[0], ground.ink[1], ground.ink[2], 1.0],
            modes: [shading, u32::from(!format.is_srgb()), 0, 0],
        }
    }
}
