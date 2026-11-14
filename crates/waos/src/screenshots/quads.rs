//! Where each picture of a screenshot lies: a quad over its rectangle, which the transforms above
//! it take to the screenshot's pixels, cut by the clips above it, each in the coordinates the
//! transforms outside it lead to, with its picture filling the rectangle or kept whole at its
//! shape, as the shell shows it ([`Fit`]), turned and mirrored as a video's frame says it is
//! shown ([`Turn`]); and its bytes, as the shader reads them (`picture.wgsl`).
//!
//! The invariants a later editor will break: a placement's mutators are read the outermost first,
//! as the WIT orders them, so a transform takes everything after it, the rectangle and the clips
//! inside it, to the coordinates outside it; and an opacity hides a layer only at none, since in
//! Waft's composition the room the picture above makes for a layer is what fades, not the layer
//! (`waft-embedder`'s `PORTING.md`), so the program's own picture carries it.

use crate::bindings::wasdk::platform::views::{Mutator, Placement, RoundedRect};
use crate::bindings::wasdk::platform::windows::{Rect, Size};

/// A 4×4 matrix, column-major, as the WIT's transforms are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Matrix(pub(super) [f32; 16]);

impl Matrix {
    pub(super) const IDENTITY: Matrix = Matrix([
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]);

    /// The matrix of a transform's 16 values, which the engine checked it has.
    fn of(values: &[f32]) -> Matrix {
        Matrix(
            values
                .try_into()
                .expect("the engine checks a transform is 16 values"),
        )
    }

    /// This matrix after `inner`: what takes a point through `inner` first, then through this.
    fn after(&self, inner: &Matrix) -> Matrix {
        let (outer, inner) = (&self.0, &inner.0);
        let mut product = [0.0; 16];
        for column in 0..4 {
            for row in 0..4 {
                product[column * 4 + row] = (0..4)
                    .map(|k| outer[k * 4 + row] * inner[column * 4 + k])
                    .sum();
            }
        }
        Matrix(product)
    }

    /// Where `point` goes, as the shader takes it: on the plane at no depth, then divided by `w`.
    #[cfg(test)]
    fn map(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        let m = &self.0;
        let w = m[3] * x + m[7] * y + m[15];
        [
            (m[0] * x + m[4] * y + m[12]) / w,
            (m[1] * x + m[5] * y + m[13]) / w,
        ]
    }
}

/// How a picture fills its layer's rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fit {
    /// Stretched over all of it: a buffer presented to the layer, which is the rectangle's size
    /// where its producer drew it at the view's pixel ratio, as the WIT says a buffer is taken.
    // TODO: a buffer of another size shows on screen at the pixel ratio from the rectangle's
    // top-left, not stretched; lay it here the same way.
    Fill,
    /// Whole, keeping its shape, centred: the system's picture of a video or a camera, as its
    /// player's layer shows one.
    Contain,
}

/// How a picture is shown: turned `rotation` degrees clockwise, then mirrored left to right where
/// `flip`, as a video's frame says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Turn {
    pub(super) rotation: u32,
    pub(super) flip: bool,
}

impl Turn {
    /// A picture of `size` as it is shown: its sides swapped by a quarter turn.
    fn size_of(self, [width, height]: [u32; 2]) -> [u32; 2] {
        if self.rotation % 180 == 90 {
            [height, width]
        } else {
            [width, height]
        }
    }
}

/// One picture's quad.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Quad {
    /// Takes the quad's own coordinates to the screenshot's pixels.
    pub(super) to_target: Matrix,
    /// The quad, in its own coordinates: left, top, right, bottom.
    pub(super) bounds: [f32; 4],
    /// Where the picture lies in it, filling that rectangle: left, top, right, bottom.
    pub(super) picture: [f32; 4],
    /// How the picture is shown there.
    pub(super) turn: Turn,
    /// What cuts it, the outermost first: what shows lies inside every one.
    pub(super) clips: Vec<Clip>,
}

/// One clip above a quad.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Clip {
    /// Takes the quad's own coordinates to the clip's.
    pub(super) to_clip: Matrix,
    /// Left, top, right, bottom.
    pub(super) bounds: [f32; 4],
    /// Each corner's horizontal and vertical radius: top-left, top-right, bottom-right,
    /// bottom-left. Zero for a rectangle's.
    pub(super) radii: [[f32; 2]; 4],
}

impl Quad {
    /// The program's drawing, `width` by `height` pixels, from the screenshot's top-left corner at
    /// its own size, as the shell shows a view's canvas.
    pub(super) fn drawing([width, height]: [u32; 2]) -> Quad {
        let bounds = [0.0, 0.0, width as f32, height as f32];
        Quad {
            to_target: Matrix::IDENTITY,
            bounds,
            picture: bounds,
            turn: Turn::default(),
            clips: Vec::new(),
        }
    }

    /// A layer `placement` puts in the program's picture, whose pixels are the screenshot's,
    /// showing a picture of `size` pixels as `fit` and `turn` say; none for one nothing lets show:
    /// an empty rectangle or picture, or an opacity of none above it.
    pub(super) fn layer(
        placement: &Placement,
        size: [u32; 2],
        fit: Fit,
        turn: Turn,
    ) -> Option<Quad> {
        let bounds = bounds_of(&placement.rect);
        let hidden = placement
            .mutators
            .iter()
            .any(|mutator| matches!(mutator, Mutator::Opacity(0)));
        if hidden || is_empty(bounds) || size.contains(&0) {
            return None;
        }
        let (to_target, clips) = transforms_and_clips(&placement.mutators);
        Some(Quad {
            to_target,
            bounds,
            picture: picture_in(bounds, turn.size_of(size), fit),
            turn,
            clips,
        })
    }
}

/// What every transform among `mutators` does together, and each clip, the outermost first, with
/// what takes the quad's coordinates to its own: the transforms after it, inside it.
fn transforms_and_clips(mutators: &[Mutator]) -> (Matrix, Vec<Clip>) {
    let mut inside = Matrix::IDENTITY;
    let mut clips = Vec::new();
    for mutator in mutators.iter().rev() {
        match mutator {
            Mutator::Transform(values) => inside = Matrix::of(values).after(&inside),
            Mutator::ClipRect(rect) => clips.push(Clip {
                to_clip: inside,
                bounds: bounds_of(rect),
                radii: [[0.0; 2]; 4],
            }),
            Mutator::ClipRoundedRect(rounded) => clips.push(rounded_clip(rounded, inside)),
            Mutator::Opacity(_) => {}
        }
    }
    clips.reverse();
    (inside, clips)
}

fn rounded_clip(rounded: &RoundedRect, to_clip: Matrix) -> Clip {
    let radii = [
        rounded.top_left,
        rounded.top_right,
        rounded.bottom_right,
        rounded.bottom_left,
    ]
    .map(|Size { width, height }| [width as f32, height as f32]);
    Clip {
        to_clip,
        bounds: bounds_of(&rounded.rect),
        radii,
    }
}

/// Where a picture of `size` lies in `bounds` as `fit` says.
fn picture_in(bounds: [f32; 4], [width, height]: [u32; 2], fit: Fit) -> [f32; 4] {
    match fit {
        Fit::Fill => bounds,
        Fit::Contain => {
            let [left, top, right, bottom] = bounds;
            let (room_width, room_height) = (right - left, bottom - top);
            let scale = (room_width / width as f32).min(room_height / height as f32);
            let (shown_width, shown_height) = (width as f32 * scale, height as f32 * scale);
            let shown_left = left + (room_width - shown_width) / 2.0;
            let shown_top = top + (room_height - shown_height) / 2.0;
            [
                shown_left,
                shown_top,
                shown_left + shown_width,
                shown_top + shown_height,
            ]
        }
    }
}

fn bounds_of(rect: &Rect) -> [f32; 4] {
    let (left, top) = (rect.origin.x as f32, rect.origin.y as f32);
    [
        left,
        top,
        left + rect.size.width as f32,
        top + rect.size.height as f32,
    ]
}

fn is_empty([left, top, right, bottom]: [f32; 4]) -> bool {
    right <= left || bottom <= top
}

/// The bytes of `quad`, to be drawn into a screenshot of `target_size` pixels, as the shader's
/// `Quad` lays them out: its fields at their offsets, then its clips, at least one, since a
/// binding holds its array's first element whether or not it is read.
pub(super) fn bytes_of(quad: &Quad, [width, height]: [u32; 2]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(QUAD_SIZE + CLIP_SIZE * quad.clips.len().max(1));
    let target_size = [width as f32, height as f32];
    let floats = quad
        .to_target
        .0
        .iter()
        .chain(&quad.bounds)
        .chain(&quad.picture)
        .chain(&target_size);
    for float in floats {
        bytes.extend(float.to_le_bytes());
    }
    let clip_count = u32::try_from(quad.clips.len()).expect("a placement's mutators fit a u32");
    bytes.extend(clip_count.to_le_bytes());
    bytes.extend((quad.turn.rotation % 360 / 90).to_le_bytes());
    bytes.extend(u32::from(quad.turn.flip).to_le_bytes());
    bytes.resize(QUAD_SIZE, 0);
    for clip in &quad.clips {
        let [top_left, top_right, bottom_right, bottom_left] = clip.radii;
        let floats = clip
            .to_clip
            .0
            .iter()
            .chain(&clip.bounds)
            .chain(&top_left)
            .chain(&top_right)
            .chain(&bottom_right)
            .chain(&bottom_left);
        for float in floats {
            bytes.extend(float.to_le_bytes());
        }
    }
    bytes.resize(bytes.len().max(QUAD_SIZE + CLIP_SIZE), 0);
    bytes
}

/// The bytes of the shader's `Quad` before its clips, which start at a multiple of 16.
const QUAD_SIZE: usize = 128;

/// The bytes of one of the shader's `Clip`s: a matrix and three vectors of four.
const CLIP_SIZE: usize = 112;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::wasdk::platform::windows::Point;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            origin: Point { x, y },
            size: Size { width, height },
        }
    }

    fn scale(factor: f32) -> Mutator {
        Mutator::Transform(vec![
            factor, 0.0, 0.0, 0.0, //
            0.0, factor, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    fn translation(x: f32, y: f32) -> Mutator {
        Mutator::Transform(vec![
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            x, y, 0.0, 1.0,
        ])
    }

    fn placed(rect: Rect, mutators: Vec<Mutator>) -> Placement {
        Placement { rect, mutators }
    }

    #[test]
    fn the_drawing_lies_at_its_own_size_from_the_top_left() {
        let quad = Quad::drawing([40, 30]);
        assert_eq!(quad.to_target, Matrix::IDENTITY);
        assert_eq!(quad.bounds, [0.0, 0.0, 40.0, 30.0]);
        assert_eq!(quad.picture, quad.bounds);
        assert!(quad.clips.is_empty());
    }

    #[test]
    fn the_transforms_take_the_rectangle_to_the_pictures_pixels_the_outermost_last() {
        let placement = placed(
            rect(1.0, 2.0, 10.0, 10.0),
            vec![translation(100.0, 0.0), scale(2.0)],
        );
        let quad = Quad::layer(&placement, [10, 10], Fit::Fill, Turn::default()).unwrap();
        assert_eq!(quad.to_target.map([1.0, 2.0]), [102.0, 4.0]);
        assert_eq!(quad.to_target.map([11.0, 12.0]), [122.0, 24.0]);
    }

    #[test]
    fn a_clip_is_in_the_coordinates_the_transforms_before_it_lead_to() {
        let placement = placed(
            rect(0.0, 0.0, 640.0, 360.0),
            vec![
                Mutator::ClipRect(rect(0.0, 0.0, 1000.0, 500.0)),
                scale(2.0),
                Mutator::ClipRect(rect(0.0, 0.0, 600.0, 300.0)),
            ],
        );
        let quad = Quad::layer(&placement, [640, 360], Fit::Fill, Turn::default()).unwrap();
        let [outer, inner] = &quad.clips[..] else {
            panic!("two clips: {:?}", quad.clips);
        };
        assert_eq!(inner.to_clip, Matrix::IDENTITY, "inside the scale");
        assert_eq!(inner.bounds, [0.0, 0.0, 600.0, 300.0]);
        assert_eq!(outer.to_clip.map([300.0, 100.0]), [600.0, 200.0]);
        assert_eq!(outer.bounds, [0.0, 0.0, 1000.0, 500.0]);
    }

    #[test]
    fn a_rounded_clip_keeps_each_corners_radii() {
        let corner = |width| Size { width, height: 2.0 };
        let placement = placed(
            rect(0.0, 0.0, 10.0, 10.0),
            vec![Mutator::ClipRoundedRect(RoundedRect {
                rect: rect(1.0, 1.0, 8.0, 8.0),
                top_left: corner(1.0),
                top_right: corner(2.0),
                bottom_right: corner(3.0),
                bottom_left: corner(4.0),
            })],
        );
        let quad = Quad::layer(&placement, [10, 10], Fit::Fill, Turn::default()).unwrap();
        assert_eq!(
            quad.clips[0].radii,
            [[1.0, 2.0], [2.0, 2.0], [3.0, 2.0], [4.0, 2.0]]
        );
        assert_eq!(quad.clips[0].bounds, [1.0, 1.0, 9.0, 9.0]);
    }

    #[test]
    fn an_opacity_hides_a_layer_only_at_none() {
        let at = |alpha| placed(rect(0.0, 0.0, 10.0, 10.0), vec![Mutator::Opacity(alpha)]);
        assert!(Quad::layer(&at(0), [10, 10], Fit::Fill, Turn::default()).is_none());
        let half = Quad::layer(&at(128), [10, 10], Fit::Fill, Turn::default()).unwrap();
        assert!(half.clips.is_empty(), "shown whole");
    }

    #[test]
    fn nothing_shows_of_an_empty_rectangle_or_picture() {
        let empty = placed(rect(0.0, 0.0, 0.0, 10.0), Vec::new());
        assert!(Quad::layer(&empty, [10, 10], Fit::Fill, Turn::default()).is_none());
        let some = placed(rect(0.0, 0.0, 10.0, 10.0), Vec::new());
        assert!(Quad::layer(&some, [0, 10], Fit::Fill, Turn::default()).is_none());
    }

    #[test]
    fn a_buffer_fills_its_layer_and_a_frame_keeps_its_shape_centred() {
        let placement = placed(rect(10.0, 0.0, 96.0, 48.0), Vec::new());
        let filled = Quad::layer(&placement, [48, 48], Fit::Fill, Turn::default()).unwrap();
        assert_eq!(filled.picture, [10.0, 0.0, 106.0, 48.0]);
        let contained = Quad::layer(&placement, [48, 48], Fit::Contain, Turn::default()).unwrap();
        assert_eq!(contained.picture, [34.0, 0.0, 82.0, 48.0]);
        let tall = placed(rect(0.0, 0.0, 48.0, 96.0), Vec::new());
        let contained = Quad::layer(&tall, [64, 48], Fit::Contain, Turn::default()).unwrap();
        assert_eq!(contained.picture, [0.0, 30.0, 48.0, 66.0]);
    }

    #[test]
    fn the_bytes_lay_a_quad_out_as_the_shader_reads_it() {
        let placement = placed(
            rect(0.0, 0.0, 10.0, 10.0),
            vec![Mutator::ClipRect(rect(1.0, 2.0, 3.0, 4.0))],
        );
        let quad = Quad::layer(&placement, [10, 10], Fit::Fill, Turn::default()).unwrap();
        let bytes = bytes_of(&quad, [200, 100]);
        assert_eq!(bytes.len(), QUAD_SIZE + CLIP_SIZE);
        let float = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!((float(96), float(100)), (200.0, 100.0), "the target's size");
        assert_eq!(&bytes[104..108], 1u32.to_le_bytes(), "one clip");
        assert_eq!(float(QUAD_SIZE + 64), 1.0, "the clip's left");
        assert_eq!(float(QUAD_SIZE + 76), 6.0, "the clip's bottom");

        let unclipped = bytes_of(&Quad::drawing([10, 10]), [10, 10]);
        assert_eq!(
            unclipped.len(),
            QUAD_SIZE + CLIP_SIZE,
            "room for the clip a binding holds"
        );
        assert_eq!(&unclipped[104..108], 0u32.to_le_bytes());
    }

    /// A camera's 640 by 480 frame shown turned a quarter and mirrored, as a phone's front camera
    /// shows: kept whole at its turned shape, and its turn in the bytes the shader reads.
    #[test]
    fn a_turned_frame_lies_at_its_turned_shape_and_says_its_turn() {
        let placement = placed(rect(0.0, 0.0, 300.0, 400.0), Vec::new());
        let turn = Turn {
            rotation: 270,
            flip: true,
        };
        let quad = Quad::layer(&placement, [640, 480], Fit::Contain, turn).unwrap();
        assert_eq!(
            quad.picture,
            [0.0, 0.0, 300.0, 400.0],
            "480 by 640 fills it"
        );
        let bytes = bytes_of(&quad, [300, 400]);
        assert_eq!(&bytes[108..112], 3u32.to_le_bytes(), "three quarter turns");
        assert_eq!(&bytes[112..116], 1u32.to_le_bytes(), "mirrored");
    }
}
