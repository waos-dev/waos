//! A key pressed in a session's window: one of the terminal's own, which scrolls, copies, pastes
//! or opens a window (`chords`), or one typed into the shell as the bytes a terminal sends for
//! it. A key that types a character is left to the platform, which enters it through the view's
//! text input (`text_entry`), so a key's text and an input method's reach the shell the same way.

use waft_embedder::TargetPlatform;
use waft_foundation::{App, Entity};
use waft_services::{KeyEvent, LogicalKeyboardKey as Key};
use waft_widgets::{Actions, AnyFocusNode, KeyEventResult};

use crate::chords::{TerminalCommand, command_for_key};
use crate::clipboard_commands::{copy_selection, paste};
use crate::grid_painter::TextSettings;
use crate::keyboard::held_modifiers;
use crate::keys::{Modifiers, bytes_for_key};
use crate::session::{Session, Stage, clear, scroll_by, start, type_bytes};
use crate::text_size::step_text_size;
use crate::window::{CloseWindowIntent, open_window};

/// What a window's keys act on: its session, and the text every window is drawn in.
#[derive(Clone)]
pub(crate) struct KeyTarget {
    pub(crate) session: Entity<Session>,
    pub(crate) text: Entity<TextSettings>,
}

/// Does what the key of `event` does in the window of `target`, pressed while `focus` has the
/// keyboard, and says whether it did anything.
pub(crate) fn key_pressed(
    app: &mut App,
    target: &KeyTarget,
    focus: AnyFocusNode,
    event: &KeyEvent,
) -> KeyEventResult {
    if matches!(event, KeyEvent::Up(_)) || is_composing(app, &target.session) {
        return KeyEventResult::Ignored;
    }
    let key = event.logical_key();
    let modifiers = held_modifiers(app);
    let handled = starts_again(app, &target.session, key)
        || performs_command(app, target, focus, key, modifiers)
        || types(app, &target.session, key, modifiers);
    if handled {
        KeyEventResult::Handled
    } else {
        KeyEventResult::Ignored
    }
}

/// While an input method composes, every key is its own: Enter commits, the arrows move among
/// its candidates.
fn is_composing(app: &App, session: &Entity<Session>) -> bool {
    !session.read(app).composing.is_empty()
}

/// Enter in a session whose shell ended or did not start starts the shell again.
fn starts_again(app: &mut App, session: &Entity<Session>, key: Key) -> bool {
    let over = matches!(
        session.read(app).stage,
        Stage::Left | Stage::Ended(_) | Stage::Failed(_)
    );
    let starts = over && matches!(key, Key::ENTER | Key::NUMPAD_ENTER);
    if starts {
        start(app, session);
    }
    starts
}

/// Does what one of the terminal's own keys asks, if `key` is one.
fn performs_command(
    app: &mut App,
    target: &KeyTarget,
    focus: AnyFocusNode,
    key: Key,
    modifiers: Modifiers,
) -> bool {
    let Some(command) = command_for_key(key, modifiers) else {
        return false;
    };
    let session = &target.session;
    let page = isize::try_from(session.read(app).screen.size().rows).unwrap_or(1);
    match command {
        TerminalCommand::Copy => copy_selection(app, session),
        TerminalCommand::Paste => paste(app, session),
        TerminalCommand::Clear => clear(app, session),
        TerminalCommand::NewWindow => open_window(app),
        TerminalCommand::CloseWindow => close_window(app, focus),
        TerminalCommand::SizeText(step) => step_text_size(app, &target.text, step),
        TerminalCommand::PageUp => scroll_by(app, session, page),
        TerminalCommand::PageDown => scroll_by(app, session, -page),
    }
    true
}

/// Asks the window `focus` is in to close, as the menu's Close Window does.
fn close_window(app: &mut App, focus: AnyFocusNode) {
    if let Some(context) = focus.context(app) {
        Actions::maybe_invoke(app, context, &CloseWindowIntent);
    }
}

/// Types the bytes the key sends, if it sends any.
fn types(app: &mut App, session: &Entity<Session>, key: Key, modifiers: Modifiers) -> bool {
    let alt_sends_escape = !alt_types_characters(app);
    let state = session.read(app);
    if !state.is_running() {
        return false;
    }
    let modes = state.screen.key_modes(alt_sends_escape);
    match bytes_for_key(key, modifiers, modes) {
        Some(bytes) => {
            type_bytes(app, session, &bytes);
            true
        }
        None => false,
    }
}

/// On Apple's keyboards Option is how other characters are typed, so it is not Meta there.
fn alt_types_characters(app: &App) -> bool {
    matches!(
        app.platform().target_platform(),
        TargetPlatform::MacOS | TargetPlatform::IOS
    )
}
