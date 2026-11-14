//! The camera: it looks at a point of the model from a distance, and orbits it.

use std::f32::consts::FRAC_PI_2;

use glam::{Mat4, Vec2, Vec3};

use crate::model::bounds::Bounds;

/// The camera's vertical field of view.
const FIELD_OF_VIEW: f32 = 0.6;
/// How far the camera may tilt up or down: just short of straight above and below.
const STEEPEST: f32 = FRAC_PI_2 - 0.01;
/// Radians turned for each point the pointer is dragged.
const TURN_PER_POINT: f32 = 0.008;

#[derive(Clone, Copy)]
pub struct Camera {
    target: Vec3,
    /// From the target to the camera, as a multiple of `scale`.
    distance: f32,
    /// The angle around the y axis, and the angle above the horizon.
    turn: f32,
    tilt: f32,
    /// The framed model's radius: what distances are measured in, so a model of any size is
    /// moved around alike.
    scale: f32,
}

impl Camera {
    /// A camera that shows all of `bounds`, a little from the side and from above.
    pub fn framing(bounds: &Bounds) -> Camera {
        Camera {
            target: bounds.centre(),
            distance: 1.15 / (FIELD_OF_VIEW * 0.5).sin(),
            turn: 0.6,
            tilt: 0.32,
            scale: bounds.radius(),
        }
    }

    pub fn position(&self) -> Vec3 {
        self.target + self.backward() * self.distance * self.scale
    }

    /// The matrix from the model's space to clip space, for a picture `aspect` times as wide as
    /// it is tall. The planes are set around the model, and far enough for its ground.
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        let reach = self.distance * self.scale;
        let near = ((reach - self.scale * 2.0) * 0.5).max(self.scale * 0.01);
        let far = reach + self.scale * 60.0;
        // A picture taller than wide keeps the model in sight by widening what it sees upwards.
        let field_of_view = if aspect < 1.0 {
            2.0 * ((FIELD_OF_VIEW * 0.5).tan() / aspect).atan()
        } else {
            FIELD_OF_VIEW
        };
        let projection = Mat4::perspective_rh(field_of_view, aspect, near, far);
        projection * Mat4::look_at_rh(self.position(), self.target, Vec3::Y)
    }

    /// Turns around the target as a drag of `points` asks.
    pub fn orbit(&mut self, points: Vec2) {
        self.turn -= points.x * TURN_PER_POINT;
        self.tilt = (self.tilt + points.y * TURN_PER_POINT).clamp(-STEEPEST, STEEPEST);
    }

    /// Slides the target with a drag of `points` in a picture `height` points tall, so that
    /// what is at the target's depth stays under the pointer.
    pub fn pan(&mut self, points: Vec2, height: f32) {
        let per_point = 2.0 * (FIELD_OF_VIEW * 0.5).tan() * self.distance * self.scale / height;
        let backward = self.backward();
        let right = Vec3::Y.cross(backward).normalize();
        let up = backward.cross(right);
        self.target += (up * points.y - right * points.x) * per_point;
    }

    /// Moves towards the target for a `factor` above one, and away for one below.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance / factor).clamp(0.05, 40.0);
    }

    /// The direction from the target to the camera.
    fn backward(&self) -> Vec3 {
        let (sin_turn, cos_turn) = self.turn.sin_cos();
        let (sin_tilt, cos_tilt) = self.tilt.sin_cos();
        Vec3::new(sin_turn * cos_tilt, sin_tilt, cos_turn * cos_tilt)
    }
}
