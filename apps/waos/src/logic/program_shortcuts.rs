//! Each installed program's shortcut, where the system keeps an app's shortcuts: on Android, an
//! icon of its own on the home screen, pinned, and an entry on WaOS's icon's menu of recent
//! programs, dynamic. A shortcut goes by its program's id, so a recent program the user drags onto
//! the home screen is the same shortcut pinned. A tap opens the program where programs open: in a
//! task of its own, as its first window, or inside WaOS, which then opens the program's tab
//! (`ShortcutOpens`). What a system keeps is made to match what is installed (`changes_for`): a
//! removed program's shortcut is disabled, since no app may remove a pinned one, an installed
//! program's is enabled and updated to its name, icon and place.
//!
//! The invariant a later editor will break: what [`action_of`] writes, [`program_of_action`]
//! reads back, and every shortcut's id is its program's id, so a shortcut whose id names no
//! installed program is disabled and nothing else is.

use std::collections::BTreeSet;

use futures::future::LocalBoxFuture;
use waft::ui::IconImage;
use waos_package::ProgramId;

use super::program_faces::icon_of;

/// What a tap on a program's shortcut opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShortcutOpens {
    /// The program's first window, a task of its own.
    OwnTask,
    /// WaOS's own window, with the program in its tab.
    InsideWaos,
}

/// A program's shortcut, as WaOS gives it to the system.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProgramShortcut {
    pub(crate) id: ProgramId,
    /// Its name as its user knows it.
    pub(crate) name: String,
    /// Its package's icon as an adaptive icon's layer (`shortcut_icon`); none where the icon
    /// cannot be read.
    pub(crate) icon: Option<IconImage>,
    pub(crate) opens: ShortcutOpens,
}

/// A shortcut the system keeps of WaOS's, as it reads back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeptShortcut {
    pub(crate) id: String,
    /// Whether it is on the home screen.
    pub(crate) pinned: bool,
    pub(crate) enabled: bool,
    /// Whether WaOS disabled it, rather than the system, as after a restore to another phone.
    pub(crate) disabled_by_waos: bool,
}

/// What makes the shortcuts a system keeps match what is installed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ShortcutChanges {
    /// Shortcuts WaOS disabled whose program is installed again.
    pub(crate) enable: Vec<ProgramId>,
    /// Every shortcut of an installed program, to its name, icon and place now.
    pub(crate) update: Vec<ProgramId>,
    /// Enabled shortcuts whose id names no installed program.
    pub(crate) disable: Vec<String>,
}

/// The changes that make `kept` match the programs `installed`. A shortcut the system disabled
/// itself is left: Android says why, and enables it once that holds no more.
pub(crate) fn changes_for(
    kept: &[KeptShortcut],
    installed: &BTreeSet<ProgramId>,
) -> ShortcutChanges {
    let mut changes = ShortcutChanges::default();
    for shortcut in kept {
        let program = shortcut
            .id
            .parse::<ProgramId>()
            .ok()
            .filter(|id| installed.contains(id));
        match program {
            Some(id) if shortcut.enabled => changes.update.push(id),
            Some(id) if shortcut.disabled_by_waos => {
                changes.enable.push(id.clone());
                changes.update.push(id);
            }
            Some(_) => {}
            None if shortcut.enabled => changes.disable.push(shortcut.id.clone()),
            None => {}
        }
    }
    changes
}

/// What every action of a shortcut opening inside WaOS starts with, so no other is taken for one.
#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "tested on every system, used on Android")
)]
const ACTION_PREFIX: &str = "waos-open-program:";

/// The action a shortcut that opens the program `id` inside WaOS carries.
#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "tested on every system, used on Android")
)]
pub(crate) fn action_of(id: &ProgramId) -> String {
    format!("{ACTION_PREFIX}{id}")
}

/// The program a shortcut's `action` opens inside WaOS; none where it names none.
#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "tested on every system, used on Android")
)]
pub(crate) fn program_of_action(action: &str) -> Option<ProgramId> {
    action.strip_prefix(ACTION_PREFIX)?.parse().ok()
}

/// The icon in `png`, a package's full square, as an adaptive icon's layer whose visible middle is
/// `visible` pixels a side; none where it cannot be read, which is said. Decoding and scaling take
/// a while for a large icon, so this runs off the main thread.
pub(crate) fn shortcut_icon(png: &[u8], visible: u32) -> Option<IconImage> {
    match icon_of(png) {
        Ok(icon) => Some(adaptive_layer(&icon, visible)),
        Err(error) => {
            tracing::warn!(%error, "a program's icon cannot be read for its shortcut");
            None
        }
    }
}

/// What `icon`, a full square, is as an adaptive icon's layer whose visible middle is `visible`
/// pixels a side: the square fills that middle, which the launcher cuts to its own shape, and its
/// edge pixels go on out to the layer's edge, half as far again, which the launcher shows only as
/// the icon moves.
pub(crate) fn adaptive_layer(icon: &IconImage, visible: u32) -> IconImage {
    let visible = visible.max(1);
    let side = visible * 3 / 2;
    let inset = (side - visible) / 2;
    let scaled = scaled_square(icon, visible);
    let mut rgba = Vec::with_capacity(side as usize * side as usize * 4);
    for y in 0..side {
        let from_y = y.saturating_sub(inset).min(visible - 1);
        for x in 0..side {
            let from_x = x.saturating_sub(inset).min(visible - 1);
            rgba.extend_from_slice(&scaled.get_pixel(from_x, from_y).0);
        }
    }
    IconImage {
        width: side,
        height: side,
        rgba: rgba.into(),
        template: false,
    }
}

/// `icon` scaled to a square `side` pixels a side.
fn scaled_square(icon: &IconImage, side: u32) -> image::RgbaImage {
    let source = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba.to_vec())
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    image::imageops::resize(&source, side, side, image::imageops::FilterType::Lanczos3)
}

/// Where WaOS keeps its programs' shortcuts: Android's `ShortcutManager`, through Waft's Android
/// host. Each call is the system's own; what fails is said in the answer or the log.
pub(crate) trait SystemShortcuts {
    /// Whether the launcher pins a shortcut on request now; it changes with the default launcher.
    fn pins(&self) -> bool;

    /// The side, in pixels, of the visible middle of a shortcut's icon as the launcher shows it.
    fn icon_size(&self) -> u32;

    /// Asks the launcher to pin `shortcut`, which asks the user: answered once the launcher has
    /// the request, never with the user's answer, which Android does not tell.
    fn request_pin(
        &self,
        shortcut: &ProgramShortcut,
    ) -> LocalBoxFuture<'static, Result<(), String>>;

    /// Puts `shortcut` first among the recent programs on WaOS's icon's menu, the oldest leaving
    /// when it is full, and tells the launcher it was used, which it ranks shortcuts by.
    fn add_recent(&self, shortcut: &ProgramShortcut);

    /// The shortcuts the system keeps of WaOS's, pinned or recent.
    fn kept(&self) -> LocalBoxFuture<'static, Result<Vec<KeptShortcut>, String>>;

    /// Changes each of `shortcuts`, pinned or recent, to its name, icon and place now; an error
    /// where Android limits WaOS in the background, to try again once WaOS is in front.
    fn update(&self, shortcuts: &[ProgramShortcut]) -> LocalBoxFuture<'static, Result<(), String>>;

    /// Enables the shortcuts of `ids` WaOS disabled.
    fn enable(&self, ids: &[ProgramId]);

    /// Takes the shortcuts of `ids` off WaOS's icon's menu, and greys out the pinned ones, which
    /// say their program was removed when tapped.
    fn disable(&self, ids: &[String]);

    /// From now on `tapped` hears the program each tap on a shortcut opening inside WaOS names,
    /// the tap that started WaOS among them.
    fn hear_taps(&self, tapped: Box<dyn Fn(ProgramId)>);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> ProgramId {
        text.parse().unwrap()
    }

    fn kept(id: &str, enabled: bool, disabled_by_waos: bool) -> KeptShortcut {
        KeptShortcut {
            id: id.to_owned(),
            pinned: true,
            enabled,
            disabled_by_waos,
        }
    }

    #[test]
    fn an_action_names_its_program_again() {
        let notes = id("dev.waos.notes");
        assert_eq!(program_of_action(&action_of(&notes)), Some(notes));
    }

    #[test]
    fn other_actions_name_no_program() {
        for other in [
            "",
            "dev.waos.notes",
            "waos-open-program:",
            "open:dev.waos.notes",
        ] {
            assert_eq!(program_of_action(other), None, "{other:?}");
        }
    }

    #[test]
    fn installed_programs_are_updated_and_removed_ones_disabled() {
        let installed = BTreeSet::from([id("dev.waos.notes")]);
        let changes = changes_for(
            &[
                kept("dev.waos.notes", true, false),
                kept("dev.example.gone", true, false),
                kept("not a program id", true, false),
            ],
            &installed,
        );
        assert_eq!(
            changes,
            ShortcutChanges {
                enable: Vec::new(),
                update: vec![id("dev.waos.notes")],
                disable: vec!["dev.example.gone".to_owned(), "not a program id".to_owned()],
            }
        );
    }

    #[test]
    fn a_program_installed_again_is_enabled_and_updated() {
        let installed = BTreeSet::from([id("dev.waos.notes")]);
        let changes = changes_for(&[kept("dev.waos.notes", false, true)], &installed);
        assert_eq!(changes.enable, [id("dev.waos.notes")]);
        assert_eq!(changes.update, [id("dev.waos.notes")]);
    }

    #[test]
    fn what_the_system_disabled_itself_or_is_disabled_already_is_left() {
        let installed = BTreeSet::from([id("dev.waos.notes")]);
        let changes = changes_for(
            &[
                kept("dev.waos.notes", false, false),
                kept("dev.example.gone", false, true),
            ],
            &installed,
        );
        assert_eq!(changes, ShortcutChanges::default());
    }

    #[test]
    fn an_adaptive_layer_puts_the_square_in_its_middle_and_its_edges_beyond() {
        let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
        // A square red on its left half and blue on its right, scaled down to half its side.
        let rgba: Vec<u8> = (0..80)
            .flat_map(|_| (0..80).map(|x| if x < 40 { red } else { blue }))
            .flatten()
            .collect();
        let icon = IconImage {
            width: 80,
            height: 80,
            rgba: rgba.into(),
            template: false,
        };
        let layer = adaptive_layer(&icon, 40);
        assert_eq!((layer.width, layer.height), (60, 60));
        let pixel = |x: u32, y: u32| {
            let at = ((y * layer.width + x) * 4) as usize;
            [
                layer.rgba[at],
                layer.rgba[at + 1],
                layer.rgba[at + 2],
                layer.rgba[at + 3],
            ]
        };
        assert_eq!(pixel(0, 0), red, "the left edge goes on out");
        assert_eq!(pixel(59, 59), blue, "and the right");
        assert_eq!(pixel(12, 30), red, "the middle is the square");
        assert_eq!(pixel(47, 30), blue);
    }
}
