//! Where a frame places a composition layer, in the WIT's shape: Waft's [`Placement`], Flutter's
//! `EmbeddedViewParams`, carried mutator for mutator in the order the scene gives them, the
//! outermost first. A transform crosses as its 16 values, column-major, as Flutter's arrays are;
//! a path clip crosses as the rectangle that bounds the path, which the WIT carries where it has
//! no paths (native-surfaces decision 6): the picture's room is still cut by the exact path, so
//! the bounds show the layer beyond the path only where the program painted nothing opaque.
//!
//! The invariant: nothing is dropped, merged or reordered here; a host reads the mutators in the
//! scene's order.

use waft_embedder::{Mutator, Placement, RRect, Rect};
use wasdk::views;
use wasdk::windows;

/// `placement` as the WIT carries it.
pub(crate) fn placement_of(placement: &Placement) -> views::Placement {
    views::Placement {
        rect: rect_of(placement.rect),
        mutators: placement.mutators.iter().map(mutator_of).collect(),
    }
}

fn mutator_of(mutator: &Mutator) -> views::Mutator {
    match mutator {
        Mutator::Transform(matrix) => views::Mutator::Transform(matrix.to_flutter_array().to_vec()),
        Mutator::ClipRect(rect) => views::Mutator::ClipRect(rect_of(*rect)),
        Mutator::ClipRRect(rrect) => views::Mutator::ClipRoundedRect(rounded_rect_of(*rrect)),
        Mutator::ClipPath(path) => views::Mutator::ClipRect(rect_of_valo(path.bounds())),
        Mutator::Opacity(alpha) => views::Mutator::Opacity(*alpha),
    }
}

fn rect_of(rect: Rect) -> windows::Rect {
    windows::Rect {
        origin: windows::Point {
            x: rect.left,
            y: rect.top,
        },
        size: windows::Size {
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        },
    }
}

fn rect_of_valo(rect: valo::Rect) -> windows::Rect {
    windows::Rect {
        origin: windows::Point {
            x: f64::from(rect.x),
            y: f64::from(rect.y),
        },
        size: windows::Size {
            width: f64::from(rect.width),
            height: f64::from(rect.height),
        },
    }
}

fn rounded_rect_of(rrect: RRect) -> views::RoundedRect {
    views::RoundedRect {
        rect: rect_of(Rect::from_ltrb(
            rrect.left,
            rrect.top,
            rrect.right,
            rrect.bottom,
        )),
        top_left: radii(rrect.tl_radius_x, rrect.tl_radius_y),
        top_right: radii(rrect.tr_radius_x, rrect.tr_radius_y),
        bottom_right: radii(rrect.br_radius_x, rrect.br_radius_y),
        bottom_left: radii(rrect.bl_radius_x, rrect.bl_radius_y),
    }
}

/// A corner's radii, horizontal and vertical.
fn radii(x: f64, y: f64) -> windows::Size {
    windows::Size {
        width: x,
        height: y,
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::{Matrix4, PathBuilder};

    use super::*;

    fn wit_rect(x: f64, y: f64, width: f64, height: f64) -> String {
        format!(
            "{:?}",
            windows::Rect {
                origin: windows::Point { x, y },
                size: windows::Size { width, height },
            }
        )
    }

    #[test]
    fn a_placement_keeps_its_rectangle_and_its_mutators_in_order() {
        let placement = Placement {
            rect: Rect::from_ltwh(10.0, 20.0, 300.0, 200.0),
            mutators: vec![
                Mutator::Opacity(51),
                Mutator::Transform(Matrix4::translation(5.0, 6.0)),
                Mutator::ClipRect(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0)),
            ],
        };
        let carried = placement_of(&placement);
        assert_eq!(
            format!("{:?}", carried.rect),
            wit_rect(10.0, 20.0, 300.0, 200.0)
        );
        assert!(matches!(carried.mutators[0], views::Mutator::Opacity(51)));
        let views::Mutator::Transform(matrix) = &carried.mutators[1] else {
            panic!("a transform second: {:?}", carried.mutators);
        };
        assert_eq!(
            matrix.as_slice(),
            Matrix4::translation(5.0, 6.0).to_flutter_array()
        );
        assert_eq!(
            matrix[12..14],
            [5.0, 6.0],
            "column-major: the translation last"
        );
        let views::Mutator::ClipRect(clip) = &carried.mutators[2] else {
            panic!("a rectangle clip third: {:?}", carried.mutators);
        };
        assert_eq!(format!("{clip:?}"), wit_rect(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn a_rounded_clip_keeps_each_corners_radii() {
        let rrect = RRect {
            left: 0.0,
            top: 0.0,
            right: 100.0,
            bottom: 50.0,
            tl_radius_x: 1.0,
            tl_radius_y: 2.0,
            tr_radius_x: 3.0,
            tr_radius_y: 4.0,
            br_radius_x: 5.0,
            br_radius_y: 6.0,
            bl_radius_x: 7.0,
            bl_radius_y: 8.0,
        };
        let views::Mutator::ClipRoundedRect(carried) = mutator_of(&Mutator::ClipRRect(rrect))
        else {
            panic!("a rounded clip crosses as one");
        };
        assert_eq!(
            format!("{:?}", carried.rect),
            wit_rect(0.0, 0.0, 100.0, 50.0)
        );
        let corners = [
            carried.top_left,
            carried.top_right,
            carried.bottom_right,
            carried.bottom_left,
        ]
        .map(|corner| (corner.width, corner.height));
        assert_eq!(corners, [(1.0, 2.0), (3.0, 4.0), (5.0, 6.0), (7.0, 8.0)]);
    }

    #[test]
    fn a_path_clip_crosses_as_its_bounds() {
        let mut path = PathBuilder::new();
        path.rrect(Rect::from_ltwh(10.0, 20.0, 200.0, 40.0).into(), 4.0);
        let views::Mutator::ClipRect(bounds) = mutator_of(&Mutator::ClipPath(path.build())) else {
            panic!("a path clip crosses as a rectangle clip");
        };
        assert_eq!(format!("{bounds:?}"), wit_rect(10.0, 20.0, 200.0, 40.0));
    }
}
