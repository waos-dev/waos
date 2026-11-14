//! WaOS's own icon at the size a menu bar or a tray shows it: the app's icon, `assets/icon.png`,
//! made smaller by averaging the pixels each of its own covers, as WaOS keeps an icon in the menu
//! bar or the tray in place of the Dock's or the taskbar's.
//!
//! The invariant: the icon keeps the app icon's colours and its see-through corners, since the
//! average is taken of premultiplied pixels, so a transparent pixel adds no colour.

use waft::ui::IconImage;

use super::program_faces::icon_of;

/// The app's icon, as the bundle carries it.
const APP_ICON: &[u8] = include_bytes!("../../assets/icon.png");

/// How many pixels across the tray's icon is: enough for a menu bar's 22 points at twice the
/// pixels, and for the 16 and 24 a tray takes.
const TRAY_SIDE: u32 = 44;

/// WaOS's icon for the menu bar or the tray; none where the app's icon does not read, which is
/// said.
pub(crate) fn waos_tray_image() -> Option<IconImage> {
    match icon_of(APP_ICON) {
        Ok(icon) => Some(scaled_down(&icon, TRAY_SIDE)),
        Err(error) => {
            tracing::warn!(%error, "WaOS's icon does not read, so it has no tray icon");
            None
        }
    }
}

/// `icon` made `side` pixels square, each pixel the average of those of `icon` it covers.
fn scaled_down(icon: &IconImage, side: u32) -> IconImage {
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            rgba.extend(average_over(
                icon,
                covered(x, side, icon.width),
                covered(y, side, icon.height),
            ));
        }
    }
    IconImage {
        width: side,
        height: side,
        rgba: rgba.into(),
        template: false,
    }
}

/// The pixels of a side `from` long that pixel `at` of a side `side` long covers.
fn covered(at: u32, side: u32, from: u32) -> std::ops::Range<u32> {
    let start = at * from / side;
    let end = ((at + 1) * from / side).max(start + 1);
    start..end.min(from)
}

/// The average of the pixels of `icon` in `columns` and `rows`, straight RGBA, taken over their
/// premultiplied colours.
fn average_over(
    icon: &IconImage,
    columns: std::ops::Range<u32>,
    rows: std::ops::Range<u32>,
) -> [u8; 4] {
    let mut sum = [0u64; 4];
    let mut count = 0u64;
    for y in rows {
        for x in columns.clone() {
            let at = ((y * icon.width + x) * 4) as usize;
            let [red, green, blue, alpha] =
                [0, 1, 2, 3].map(|channel| u64::from(icon.rgba[at + channel]));
            sum[0] += red * alpha;
            sum[1] += green * alpha;
            sum[2] += blue * alpha;
            sum[3] += alpha;
            count += 1;
        }
    }
    let alpha = sum[3];
    let straight = |premultiplied: u64| premultiplied.checked_div(alpha).unwrap_or(0) as u8;
    [
        straight(sum[0]),
        straight(sum[1]),
        straight(sum[2]),
        (alpha / count.max(1)) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(width: u32, height: u32, pixels: &[[u8; 4]]) -> IconImage {
        IconImage {
            width,
            height,
            rgba: pixels.concat().into(),
            template: false,
        }
    }

    #[test]
    fn waos_has_a_tray_icon_of_the_trays_size() {
        let image = waos_tray_image().expect("the app's icon reads");
        assert_eq!((image.width, image.height), (TRAY_SIDE, TRAY_SIDE));
        assert_eq!(image.rgba.len(), (TRAY_SIDE * TRAY_SIDE * 4) as usize);
    }

    #[test]
    fn each_pixel_is_the_average_of_those_it_covers() {
        let grey = icon(2, 1, &[[0, 0, 0, 255], [200, 100, 50, 255]]);
        assert_eq!(&*scaled_down(&grey, 1).rgba, [100, 50, 25, 255]);
    }

    #[test]
    fn a_transparent_pixel_adds_no_colour() {
        let half = icon(2, 1, &[[255, 0, 0, 255], [0, 0, 255, 0]]);
        assert_eq!(&*scaled_down(&half, 1).rgba, [255, 0, 0, 127]);
    }
}
