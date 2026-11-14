//! What the window says above the note or the file shown, in Fluent's info bars: why a note could
//! not be saved, while the store cannot keep it; that the note shown is deleted, with Recover; why
//! the file shown is not saved, with what the user may do of another app's change or removal; and
//! what a command did, until the user closes it or the next change. Nothing while it has nothing
//! to say.

use waft_foundation::{App, Listener};
use waft_widgets::{
    BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, IntoWidget, MainAxisSize,
    Padding, Row, SizedBox, StatelessWidget, WidgetRef,
};
use waft_winui::{Button, ButtonStyle, InfoBar, InfoBarSeverity};

use crate::editing::recover_shown;
use crate::library::KEPT_DELETED;
use crate::notes_app::NotesApp;
use crate::open_file::FileProblem;
use crate::opening::close_shown_file;
use crate::outside_changes::{revert_shown, save_shown_anyway};

/// The space between two bars, and below the last, in logical points.
const GAP: f64 = 8.0;

#[derive(Debug)]
pub(crate) struct Notices {
    pub(crate) notes: NotesApp,
}

impl StatelessWidget for Notices {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let bars: Vec<WidgetRef> = [
            unsaved_bar(app, &self.notes),
            deleted_bar(app, &self.notes),
            file_problem_bar(app, &self.notes),
            notice_bar(app, &self.notes),
        ]
        .into_iter()
        .flatten()
        .map(IntoWidget::into_widget)
        .collect();
        if bars.is_empty() {
            return SizedBox::shrink().into_widget();
        }
        let column = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(GAP)
            .children(bars);
        Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 0.0, 0.0, GAP))
            .child(column)
            .into_widget()
    }
}

/// Why a note could not be saved, while the store cannot keep it.
fn unsaved_bar(app: &App, notes: &NotesApp) -> Option<InfoBar> {
    let library = notes.library.read(app);
    let (note, why) = library.unsaved()?;
    let bar = InfoBar::new(true, |_, _| {})
        .severity(InfoBarSeverity::Warning)
        .title(format!("“{}” could not be saved.", note.title()))
        .message(format!("{why}. Notes tries again as you type."))
        .is_closable(false);
    Some(bar)
}

/// What a deleted note says of itself, with Recover.
fn deleted_bar(app: &App, notes: &NotesApp) -> Option<InfoBar> {
    let id = notes.browsing.read(app).shown_note()?;
    if !notes.library.read(app).note(id)?.is_deleted() {
        return None;
    }
    let days = KEPT_DELETED.as_secs() / (24 * 60 * 60);
    let recovering = notes.clone();
    let recover = Button::text(
        "Recover",
        Listener::new(move |app: &mut App| recover_shown(app, &recovering)),
    );
    let bar = InfoBar::new(true, |_, _| {})
        .severity(InfoBarSeverity::Informational)
        .message(format!(
            "Deleted notes can’t be changed, and go for good {days} days after they were deleted."
        ))
        .is_closable(false)
        .action_button(recover);
    Some(bar)
}

/// Why the file shown is not saved, with what the user may do: of another app's change, save over
/// it or revert to it; of its removal, save it again where it was or close it.
fn file_problem_bar(app: &App, notes: &NotesApp) -> Option<InfoBar> {
    let path = notes.browsing.read(app).shown_file()?;
    let file = notes.files.read(app).file(path)?;
    let (name, unsaved) = (file.name(), file.unsaved().is_some());
    let bar = InfoBar::new(true, |_, _| {}).is_closable(false);
    let bar = match file.problem()? {
        FileProblem::ChangedElsewhere => bar
            .severity(InfoBarSeverity::Warning)
            .title(format!("“{name}” was changed by another app."))
            .message("Your latest changes are not saved to it.")
            .action_button(choices(
                notes,
                ("Save Anyway", save_shown_anyway),
                ("Revert", revert_shown),
            )),
        FileProblem::Gone => bar
            .severity(InfoBarSeverity::Warning)
            .title(format!("“{name}” was moved or deleted by another app."))
            .message(if unsaved {
                "Your latest changes are not saved."
            } else {
                "Save it again to keep it where it was."
            })
            .action_button(choices(
                notes,
                ("Save Again", save_shown_anyway),
                ("Close File", close_shown_file),
            )),
        FileProblem::Unwritable(why) => bar
            .severity(InfoBarSeverity::Error)
            .title(format!("“{name}” could not be saved."))
            .message(format!("{why}. Notes tries again as you type.")),
    };
    Some(bar)
}

/// The two buttons of a choice: the first, the accent one, and the other, each labelled and
/// doing its command.
fn choices(
    notes: &NotesApp,
    (first, does_first): (&str, fn(&mut App, &NotesApp)),
    (other, does_other): (&str, fn(&mut App, &NotesApp)),
) -> Row {
    let (first_notes, other_notes) = (notes.clone(), notes.clone());
    let first = Button::text(
        first,
        Listener::new(move |app: &mut App| does_first(app, &first_notes)),
    )
    .style(ButtonStyle::Accent);
    let other = Button::text(
        other,
        Listener::new(move |app: &mut App| does_other(app, &other_notes)),
    );
    Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(GAP)
        .children([first.into_widget(), other.into_widget()])
}

/// What the last command did, until the user closes it.
fn notice_bar(app: &App, notes: &NotesApp) -> Option<InfoBar> {
    let notice = notes.browsing.read(app).notice()?.clone();
    let closing = notes.clone();
    let bar = InfoBar::new(true, move |app, open| {
        if !open {
            closing.browsing.update(app, |browsing, cx| {
                if browsing.clear_notice() {
                    cx.notify();
                }
            });
        }
    })
    .severity(notice.severity)
    .message(notice.message);
    Some(bar)
}
