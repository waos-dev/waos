//! Where a program places a composition layer, as Waft places one: the WIT's `placement`,
//! carried mutator for mutator into Waft's [`Placement`], in the order the program gave them, the
//! outermost first. A transform is the program's 16 values, column-major, as Flutter's arrays
//! are, which the engine checked it has.
//!
//! The invariant: nothing is dropped, merged or reordered here; Waft's host reads the mutators in
//! the program's order.

use waft::ui::{Matrix4, Mutator, Placement, RRect, Rect};
use waos::bindings::wasdk::platform::views;
use waos::bindings::wasdk::platform::windows;

/// `placement`, as a program's present gave it, as Waft places a layer.
pub(crate) fn placement_of(placement: views::Placement) -> Placement {
    Placement {
        rect: rect_of(placement.rect),
        mutators: placement.mutators.into_iter().map(mutator_of).collect(),
    }
}

fn mutator_of(mutator: views::Mutator) -> Mutator {
    match mutator {
        views::Mutator::Transform(values) => Mutator::Transform(matrix_of(&values)),
        views::Mutator::ClipRect(rect) => Mutator::ClipRect(rect_of(rect)),
        views::Mutator::ClipRoundedRect(rounded) => Mutator::ClipRRect(rrect_of(rounded)),
        views::Mutator::Opacity(alpha) => Mutator::Opacity(alpha),
    }
}

/// The 4×4 matrix of `values`, column-major.
///
/// # Panics
///
/// When `values` are not 16, which the engine traps a program for before its present reaches the
/// shell.
fn matrix_of(values: &[f32]) -> Matrix4 {
    let values: &[f32; 16] = values
        .try_into()
        .expect("the engine checks a transform is 16 values");
    Matrix4::from_flutter_array(values)
}

fn rect_of(rect: windows::Rect) -> Rect {
    Rect::from_ltwh(
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    )
}

fn rrect_of(rounded: views::RoundedRect) -> RRect {
    let rect = rect_of(rounded.rect);
    RRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
        tl_radius_x: rounded.top_left.width,
        tl_radius_y: rounded.top_left.height,
        tr_radius_x: rounded.top_right.width,
        tr_radius_y: rounded.top_right.height,
        br_radius_x: rounded.bottom_right.width,
        br_radius_y: rounded.bottom_right.height,
        bl_radius_x: rounded.bottom_left.width,
        bl_radius_y: rounded.bottom_left.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wit_rect(x: f64, y: f64, width: f64, height: f64) -> windows::Rect {
        windows::Rect {
            origin: windows::Point { x, y },
            size: windows::Size { width, height },
        }
    }

    fn corner(width: f64, height: f64) -> windows::Size {
        windows::Size { width, height }
    }

    #[test]
    fn a_placement_keeps_its_rectangle_and_every_mutator_in_order() {
        let translation = Matrix4::translation(5.0, 6.0);
        let placement = placement_of(views::Placement {
            rect: wit_rect(10.0, 20.0, 300.0, 200.0),
            mutators: vec![
                views::Mutator::Opacity(51),
                views::Mutator::Transform(translation.to_flutter_array().to_vec()),
                views::Mutator::ClipRect(wit_rect(0.0, 0.0, 100.0, 50.0)),
                views::Mutator::ClipRoundedRect(views::RoundedRect {
                    rect: wit_rect(1.0, 2.0, 30.0, 40.0),
                    top_left: corner(1.0, 2.0),
                    top_right: corner(3.0, 4.0),
                    bottom_right: corner(5.0, 6.0),
                    bottom_left: corner(7.0, 8.0),
                }),
            ],
        });
        assert_eq!(
            placement,
            Placement {
                rect: Rect::from_ltwh(10.0, 20.0, 300.0, 200.0),
                mutators: vec![
                    Mutator::Opacity(51),
                    Mutator::Transform(translation),
                    Mutator::ClipRect(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0)),
                    Mutator::ClipRRect(RRect {
                        left: 1.0,
                        top: 2.0,
                        right: 31.0,
                        bottom: 42.0,
                        tl_radius_x: 1.0,
                        tl_radius_y: 2.0,
                        tr_radius_x: 3.0,
                        tr_radius_y: 4.0,
                        br_radius_x: 5.0,
                        br_radius_y: 6.0,
                        bl_radius_x: 7.0,
                        bl_radius_y: 8.0,
                    }),
                ],
            }
        );
    }

    /// Column-major, as the WIT's transform and Flutter's arrays are: the translation is last.
    #[test]
    fn a_transform_is_read_column_major() {
        let mut values = Matrix4::IDENTITY.to_flutter_array().to_vec();
        values[12] = 5.0;
        values[13] = 6.0;
        let placement = placement_of(views::Placement {
            rect: wit_rect(0.0, 0.0, 10.0, 10.0),
            mutators: vec![views::Mutator::Transform(values)],
        });
        assert_eq!(placement.bounds(), Rect::from_ltwh(5.0, 6.0, 10.0, 10.0));
    }
}
