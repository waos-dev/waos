//! The model on show: the model itself, its copy on the GPU, where its nodes are now, the
//! animation that plays, and the camera on it.
//!
//! The invariant: `world` and the GPU's draws are always posed for `playback`, so every change of
//! the animation or its time goes through `pose`.

use glam::Mat4;

use crate::camera::Camera;
use crate::gpu::Gpu;
use crate::model::bounds::{self, Bounds};
use crate::model::{Model, pose};
use crate::render::gpu_model::GpuModel;
use crate::render::scene::SceneRenderer;

pub struct Shown {
    pub model: Model,
    pub gpu_model: GpuModel,
    pub world: Vec<Mat4>,
    /// The box around the model at rest.
    pub bounds: Bounds,
    pub camera: Camera,
    pub playback: Playback,
}

/// Which animation is on, and where in it.
#[derive(Clone, Copy, PartialEq)]
pub struct Playback {
    /// None for the model at rest.
    pub animation: Option<usize>,
    pub playing: bool,
    /// Seconds into the animation.
    pub time: f32,
}

impl Shown {
    /// Uploads `model` and frames it at rest. A model with animations starts playing its first.
    pub fn new(gpu: &Gpu, renderer: &SceneRenderer, model: Model) -> Shown {
        let world = pose::world_matrices(&model, &pose::rest(&model));
        let bounds = bounds::of_model(&model, &world);
        let playback = Playback {
            animation: (!model.animations.is_empty()).then_some(0),
            playing: true,
            time: 0.0,
        };
        let mut shown = Shown {
            gpu_model: GpuModel::new(gpu, renderer.layouts(), &model, &world),
            camera: Camera::framing(&bounds),
            model,
            world,
            bounds,
            playback,
        };
        shown.pose(&gpu.queue);
        shown
    }

    pub fn frame(&mut self) {
        self.camera = Camera::framing(&self.bounds);
    }

    /// Whether an animation is running, so that frames must keep coming.
    pub fn is_playing(&self) -> bool {
        self.playback.playing && self.playback.animation.is_some()
    }

    /// Moves a running animation on by `seconds`, around to its start at its end.
    pub fn advance(&mut self, queue: &wgpu::Queue, seconds: f32) {
        let Some(animation) = self.animation().filter(|_| self.playback.playing) else {
            return;
        };
        let duration = animation.duration();
        let time = self.playback.time + seconds;
        self.playback.time = if duration > 0.0 { time % duration } else { 0.0 };
        self.pose(queue);
    }

    /// Puts the nodes, and the GPU's draws, where `playback` says they are.
    pub fn pose(&mut self, queue: &wgpu::Queue) {
        let mut transforms = pose::rest(&self.model);
        if let Some(animation) = self.animation() {
            animation.apply(self.playback.time, &mut transforms);
        }
        self.world = pose::world_matrices(&self.model, &transforms);
        self.gpu_model.pose(queue, &self.model, &self.world);
    }

    fn animation(&self) -> Option<&crate::model::animation::Animation> {
        self.model.animations.get(self.playback.animation?)
    }
}
