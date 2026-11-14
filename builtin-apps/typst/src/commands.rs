//! The app's commands, each with the name a menu shows it by and the key that does it: File's
//! and View's, in the groups their menus show them in. The menu bar ([`crate::menus`]) and the
//! window's shortcuts ([`crate::shortcuts`]) are both made from these, so a key does the same on
//! a system with a menu bar and on one without.

use waft::ui::TargetPlatform;
use waft::{App, SingleActivator};
use waft_services::LogicalKeyboardKey as Key;

use crate::bar_button::Command;
use crate::exporting::export_pdf;
use crate::filing::{new_document, open_document, save_document, save_document_as};
use crate::quitting::ask_to_quit;
use crate::zooming::{actual_size, fit_width, show_pages, show_text, zoom_in, zoom_out};

pub(crate) struct MenuCommand {
    pub(crate) label: &'static str,
    pub(crate) chord: Option<Chord>,
    pub(crate) run: Command,
}

/// A key pressed with the platform's command key, and with Shift where it says.
#[derive(Clone, Copy)]
pub(crate) struct Chord {
    key: Key,
    shift: bool,
}

impl Chord {
    const fn of(key: Key) -> Option<Chord> {
        Some(Chord { key, shift: false })
    }

    const fn shifted(key: Key) -> Option<Chord> {
        Some(Chord { key, shift: true })
    }

    /// The chord with ⌘ on Apple's systems and Ctrl elsewhere.
    pub(crate) fn activator(self, app: &App) -> SingleActivator {
        let activator = SingleActivator::new(self.key).shift(self.shift);
        match app.platform().target_platform() {
            TargetPlatform::MacOS | TargetPlatform::IOS => activator.meta(true),
            _ => activator.control(true),
        }
    }
}

/// File's commands, a group to each line between them.
pub(crate) fn file_groups() -> Vec<Vec<MenuCommand>> {
    vec![
        vec![
            command("New", Chord::of(Key::KEY_N), new_document),
            command("Open…", Chord::of(Key::KEY_O), open_document),
        ],
        vec![
            command("Save", Chord::of(Key::KEY_S), save_document),
            command("Save As…", Chord::shifted(Key::KEY_S), save_document_as),
        ],
        vec![command("Export PDF…", Chord::of(Key::KEY_E), export_pdf)],
        vec![command("Close", Chord::of(Key::KEY_W), ask_to_quit)],
    ]
}

/// View's commands.
pub(crate) fn view_groups() -> Vec<Vec<MenuCommand>> {
    vec![
        vec![
            command("Zoom In", Chord::of(Key::EQUAL), zoom_in),
            command("Zoom Out", Chord::of(Key::MINUS), zoom_out),
            command("Actual Size", Chord::of(Key::DIGIT0), actual_size),
            command("Fit Width", Chord::of(Key::DIGIT9), fit_width),
        ],
        vec![
            command("Write", Chord::of(Key::DIGIT1), show_text),
            command("Preview", Chord::of(Key::DIGIT2), show_pages),
        ],
    ]
}

const fn command(label: &'static str, chord: Option<Chord>, run: Command) -> MenuCommand {
    MenuCommand { label, chord, run }
}
