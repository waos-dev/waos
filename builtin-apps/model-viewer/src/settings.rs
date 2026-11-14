//! How the user has chosen to see the model: what the panel sets and the renderer reads.

use glam::Vec3;

use crate::render::globals::Shading;

pub struct ViewSettings {
    pub shading: Shading,
    pub ground: bool,
    pub light: bool,
    /// Where the light stands around the model, in degrees from the front.
    pub light_turn: f32,
    /// Stops brighter or darker than the scene as lit.
    pub exposure: f32,
}

/// How high the light stands above the horizon, in radians.
const LIGHT_HEIGHT: f32 = 0.85;
const LIGHT_STRENGTH: f32 = 2.4;

impl Default for ViewSettings {
    fn default() -> ViewSettings {
        ViewSettings {
            shading: Shading::Shaded,
            ground: true,
            light: true,
            light_turn: 40.0,
            exposure: 0.0,
        }
    }
}

impl ViewSettings {
    /// The direction towards the light and its strength; none while it is off.
    pub fn light(&self) -> Option<(Vec3, f32)> {
        let (sin_turn, cos_turn) = self.light_turn.to_radians().sin_cos();
        let (sin_height, cos_height) = LIGHT_HEIGHT.sin_cos();
        let to_light = Vec3::new(sin_turn * cos_height, sin_height, cos_turn * cos_height);
        self.light.then_some((to_light, LIGHT_STRENGTH))
    }

    /// What the scene's light is multiplied by before it is tone mapped.
    pub fn exposure_scale(&self) -> f32 {
        self.exposure.exp2()
    }
}
