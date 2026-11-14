//! A model's animations: each moves, turns or scales nodes over time, from keys.

use glam::{Quat, Vec3};

use super::Transform;

pub struct Animation {
    pub name: String,
    pub channels: Vec<Channel>,
}

/// One node's translation, rotation or scale over time.
pub struct Channel {
    pub node: usize,
    pub track: Track,
}

pub enum Track {
    Translation(Keys<Vec3>),
    Rotation(Keys<Quat>),
    Scale(Keys<Vec3>),
}

/// Values at rising times. A cubic spline has three values to a time: the tangent coming in,
/// the value, and the tangent going out.
pub struct Keys<T> {
    pub times: Vec<f32>,
    pub values: Vec<T>,
    pub interpolation: Interpolation,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Interpolation {
    Step,
    Linear,
    CubicSpline,
}

impl Animation {
    /// How long the animation runs, in seconds: its last key's time.
    pub fn duration(&self) -> f32 {
        let last_key = |channel: &Channel| channel.track.times().last().copied();
        self.channels
            .iter()
            .filter_map(last_key)
            .fold(0.0, f32::max)
    }

    /// Sets the transforms of the nodes it animates to what they are at `time`.
    pub fn apply(&self, time: f32, transforms: &mut [Transform]) {
        for channel in &self.channels {
            let Some(transform) = transforms.get_mut(channel.node) else {
                continue;
            };
            match &channel.track {
                Track::Translation(keys) => keys.sample_into(time, &mut transform.translation),
                Track::Rotation(keys) => keys.sample_into(time, &mut transform.rotation),
                Track::Scale(keys) => keys.sample_into(time, &mut transform.scale),
            }
        }
    }
}

impl Track {
    fn times(&self) -> &[f32] {
        match self {
            Track::Translation(keys) | Track::Scale(keys) => &keys.times,
            Track::Rotation(keys) => &keys.times,
        }
    }
}

/// What keys hold: a value that blends towards another, and follows a cubic spline.
pub trait Animated: Copy {
    fn blend(self, next: Self, amount: f32) -> Self;
    /// The sum of four values, each scaled: a Hermite spline's point.
    fn weighted_sum(terms: [(Self, f32); 4]) -> Self;
}

impl Animated for Vec3 {
    fn blend(self, next: Vec3, amount: f32) -> Vec3 {
        self.lerp(next, amount)
    }

    fn weighted_sum(terms: [(Vec3, f32); 4]) -> Vec3 {
        terms
            .into_iter()
            .map(|(value, weight)| value * weight)
            .sum()
    }
}

impl Animated for Quat {
    fn blend(self, next: Quat, amount: f32) -> Quat {
        self.slerp(next, amount)
    }

    fn weighted_sum(terms: [(Quat, f32); 4]) -> Quat {
        let sum = terms.into_iter().fold(
            Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            |sum, (value, weight)| sum + value * weight,
        );
        sum.normalize()
    }
}

impl<T: Animated> Keys<T> {
    /// Sets `value` to the keys' value at `time`, held at the first and last key outside them;
    /// keys that are empty, or shorter than their times say, leave it alone.
    fn sample_into(&self, time: f32, value: &mut T) {
        if let Some(sampled) = self.sample(time) {
            *value = sampled;
        }
    }

    fn sample(&self, time: f32) -> Option<T> {
        let last = self.times.len().checked_sub(1)?;
        let next = self.times.partition_point(|key| *key <= time);
        if next == 0 {
            return self.value_at(0);
        }
        if next > last {
            return self.value_at(last);
        }
        let (start, end) = (self.times[next - 1], self.times[next]);
        let span = end - start;
        let amount = if span > 0.0 {
            (time - start) / span
        } else {
            0.0
        };
        match self.interpolation {
            Interpolation::Step => self.value_at(next - 1),
            Interpolation::Linear => {
                Some(self.value_at(next - 1)?.blend(self.value_at(next)?, amount))
            }
            Interpolation::CubicSpline => self.spline(next - 1, span, amount),
        }
    }

    /// The value of key `index`, whatever the interpolation keeps beside it.
    fn value_at(&self, index: usize) -> Option<T> {
        let place = match self.interpolation {
            Interpolation::CubicSpline => index * 3 + 1,
            Interpolation::Step | Interpolation::Linear => index,
        };
        self.values.get(place).copied()
    }

    /// glTF's cubic Hermite spline between key `index` and the next, `amount` of the way.
    fn spline(&self, index: usize, span: f32, amount: f32) -> Option<T> {
        let start = self.value_at(index)?;
        let end = self.value_at(index + 1)?;
        let leaving = *self.values.get(index * 3 + 2)?;
        let arriving = *self.values.get(index * 3 + 3)?;
        let (squared, cubed) = (amount * amount, amount * amount * amount);
        Some(T::weighted_sum([
            (start, 2.0 * cubed - 3.0 * squared + 1.0),
            (leaving, span * (cubed - 2.0 * squared + amount)),
            (end, -2.0 * cubed + 3.0 * squared),
            (arriving, span * (cubed - squared)),
        ]))
    }
}
