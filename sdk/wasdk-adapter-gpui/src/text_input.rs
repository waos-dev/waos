//! A window's text input: GPUI's `NSTextInputClient`-shaped input handler bridged to the view's
//! Flutter-shaped `text-input`.
//!
//! GPUI's platforms ask the input handler whenever they need the text; WaOS's input methods read
//! a copy the program pushes instead, and send each edit back as the whole new value. So after
//! each frame this pushes a window of the document around the selection, and turns each value
//! that comes back into GPUI's edits, from its difference with the value pushed. Offsets are UTF-16
//! on both sides.
//!
//! An input with no document to push, as a terminal has none (its `text_for_range` answers
//! none), gets a buffer of its own instead, as terminals keep a hidden field (Termux, SwiftTerm,
//! xterm.js): the input method composes in it, its finished text goes to the input as it arrives,
//! what it deletes of that text is Backspace, and a new line is Enter ([`Scratch`]).
//!
//! What an input asks of the input method, its corrections, suggestions, capitals and the action on
//! its Enter key, is GPUI's to say for each input (`TextInputConfiguration`), and is passed on as
//! it is: the client starts with it, and hears it again only when it changes, since configuring a
//! live input session may restart it.
//!
//! The invariant: an edit is read against the value last pushed or received, never the document
//! as it is now, since the input method edited that copy; so every value received becomes the
//! copy before the next push.

use std::ops::Range;

use gpui::{Autocapitalize, Bounds, Pixels, PlatformInputHandler, UTF16Selection};
use wasdk::views::{
    PixelRect, TextAffinity, TextCapitalization, TextEditingValue, TextInput,
    TextInputAction as PlatformAction, TextInputConfiguration, TextInputType, TextRange,
    TextSelection,
};

/// How many UTF-16 units of the document either side of the selection the input method sees:
/// the document's length is not known, as Android's `getTextBeforeCursor` does not know it.
const CONTEXT_UNITS: usize = 1000;

/// How much finished text a buffer of an input with no document keeps before it is emptied, at a
/// moment nothing is composed: what the input method may still edit, a Korean syllable it
/// recomposes, or a word it deletes into.
const SCRATCH_UNITS: usize = 256;

/// The copy of the document the input method edits: a window of it, and where it starts.
#[derive(Clone)]
struct Copy {
    start: usize,
    value: TextEditingValue,
}

impl Copy {
    /// Whether the input method would see the same: the generated types compare nothing.
    fn same_as(&self, other: &Copy) -> bool {
        let selection = |copy: &Copy| {
            copy.value
                .selection
                .as_ref()
                .map(|selection| (selection.base_offset, selection.extent_offset))
        };
        let composing = |copy: &Copy| {
            copy.value
                .composing
                .as_ref()
                .map(|range| (range.start, range.end))
        };
        self.start == other.start
            && self.value.text == other.value.text
            && selection(self) == selection(other)
            && composing(self) == composing(other)
    }
}

/// What an edit of the buffer of an input with no document does to the input, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Typing {
    /// Finished text, typed into the input.
    Text(String),
    /// What the input method is composing, shown in the input and not yet typed.
    Preview(String),
    /// The composition ended with nothing left to show.
    EndPreview,
    /// A key the edit means: Backspace for finished text it deleted, Enter for a new line.
    Key(&'static str),
}

/// The buffer the input method edits for an input with no document: the finished text the input
/// already took, and then what is being composed.
struct Scratch {
    /// The buffer as the input method last edited it, or as it was pushed.
    value: TextEditingValue,
    /// How much of the buffer's start, in UTF-16 units, the input took.
    taken: usize,
}

impl Scratch {
    fn empty() -> Scratch {
        Scratch {
            value: TextEditingValue {
                text: String::new(),
                selection: Some(caret_at(0)),
                composing: None,
            },
            taken: 0,
        }
    }

    /// Whether the buffer is to be emptied: it holds more finished text than it keeps, and
    /// nothing is composed.
    fn owes_emptying(&self) -> bool {
        self.taken > SCRATCH_UNITS && composing_of(&self.value).is_none()
    }

    /// What `value`, the input method's edit of the buffer, does to the input: finished text
    /// deleted, as Backspace for each character; finished text added, typed, each new line as
    /// Enter; and what is composed now, shown.
    fn edit(&mut self, value: TextEditingValue) -> Vec<Typing> {
        let old = utf16(&self.value.text);
        let new = utf16(&value.text);
        let composing = composing_of(&value);
        let finished = composing.as_ref().map_or(new.len(), |range| range.start);
        let taken = self.taken.min(old.len());
        let kept = old[..taken]
            .iter()
            .zip(&new[..finished])
            .take_while(|(old, new)| old == new)
            .count();
        let mut typing = Vec::new();
        let deleted = String::from_utf16_lossy(&old[kept..taken]).chars().count();
        typing.extend(std::iter::repeat_n(Typing::Key(BACKSPACE), deleted));
        let typed = String::from_utf16_lossy(&new[kept..finished]);
        for (line, text) in typed.split('\n').enumerate() {
            if line > 0 {
                typing.push(Typing::Key(ENTER));
            }
            if !text.is_empty() {
                typing.push(Typing::Text(text.to_owned()));
            }
        }
        match composing {
            Some(range) => typing.push(Typing::Preview(String::from_utf16_lossy(&new[range]))),
            None if composing_of(&self.value).is_some() => typing.push(Typing::EndPreview),
            None => {}
        }
        self.value = value;
        self.taken = finished;
        typing
    }
}

/// GPUI's names of the keys an edit means.
const BACKSPACE: &str = "backspace";
pub(crate) const ENTER: &str = "enter";

pub(crate) struct TextInputBridge {
    platform: TextInput,
    started: bool,
    /// What the client was last told the input asks of the input method.
    configuration: gpui::TextInputConfiguration,
    copy: Option<Copy>,
    /// The buffer of an input with no document, while one has the focus.
    scratch: Option<Scratch>,
    /// The caret's rectangle pushed last, as x, y, width and height.
    caret: Option<(f64, f64, f64, f64)>,
}

impl TextInputBridge {
    pub(crate) fn new(platform: TextInput) -> TextInputBridge {
        TextInputBridge {
            platform,
            started: false,
            configuration: gpui::TextInputConfiguration::default(),
            copy: None,
            scratch: None,
            caret: None,
        }
    }

    /// Brings the input method up to date with `handler`, the focused input or none, which asks
    /// `configuration` of it, at `scale` physical pixels to a logical one.
    pub(crate) fn sync(
        &mut self,
        handler: Option<&mut PlatformInputHandler>,
        configuration: &gpui::TextInputConfiguration,
        scale: f32,
    ) {
        let Some(handler) = handler else {
            self.stop();
            return;
        };
        if !handler.query_accepts_text_input() {
            self.stop();
            return;
        }
        self.sync_configuration(configuration);
        if !has_document(handler) {
            self.copy = None;
            self.sync_scratch();
            self.sync_caret(handler, 0, scale);
            return;
        }
        self.scratch = None;
        let Some(copy) = copy_of(handler) else {
            return;
        };
        if !self
            .copy
            .as_ref()
            .is_some_and(|pushed| pushed.same_as(&copy))
        {
            self.platform.set_editing_state(&copy.value);
            self.copy = Some(copy.clone());
        }
        let caret = copy
            .value
            .selection
            .as_ref()
            .map(|selection| copy.start + selection.extent_offset as usize);
        if let Some(caret) = caret {
            self.sync_caret(handler, caret, scale);
        }
    }

    /// Starts the client with `configuration` where it has none, and tells a client that has one
    /// only a configuration other than the one it was told last.
    fn sync_configuration(&mut self, configuration: &gpui::TextInputConfiguration) {
        if !self.started {
            self.platform
                .set_client(platform_configuration(configuration));
            self.platform.show();
            self.started = true;
        } else if &self.configuration != configuration {
            self.platform
                .update_config(platform_configuration(configuration));
        } else {
            return;
        }
        self.configuration = configuration.clone();
    }

    /// Pushes an empty buffer to the input method for an input with no document, where none was
    /// pushed yet or the one it edits is to be emptied.
    fn sync_scratch(&mut self) {
        if self
            .scratch
            .as_ref()
            .is_some_and(|scratch| !scratch.owes_emptying())
        {
            return;
        }
        let scratch = Scratch::empty();
        self.platform.set_editing_state(&scratch.value);
        self.scratch = Some(scratch);
    }

    /// Tells the input method where the caret is, at `caret` in the document, for its candidates.
    fn sync_caret(&mut self, handler: &mut PlatformInputHandler, caret: usize, scale: f32) {
        if let Some(caret) = caret_rect(handler, caret, scale) {
            let place = (caret.x, caret.y, caret.width, caret.height);
            if self.caret != Some(place) {
                self.platform.set_caret_rect(caret);
                self.caret = Some(place);
            }
        }
    }

    /// Applies the input method's `value` to `handler`, as GPUI's edits; for an input with no
    /// document, answers what the edit does to it instead, for the window to do in order.
    pub(crate) fn apply(
        &mut self,
        value: TextEditingValue,
        handler: &mut PlatformInputHandler,
    ) -> Vec<Typing> {
        if let Some(scratch) = &mut self.scratch {
            return scratch.edit(value);
        }
        let Some(old) = self.copy.take() else {
            return Vec::new();
        };
        let start = old.start;
        let old_text = utf16(&old.value.text);
        let new_text = utf16(&value.text);
        let caret = old
            .value
            .selection
            .as_ref()
            .map_or(old_text.len(), |selection| {
                selection.base_offset.min(selection.extent_offset) as usize
            });
        let (prefix, old_end, new_end) = changed_span(&old_text, &new_text, caret);
        let replaced = start + prefix..start + old_end;
        let inserted = String::from_utf16_lossy(&new_text[prefix..new_end]);
        match (&old.value.composing, &value.composing) {
            (_, Some(composing)) => {
                let marked = String::from_utf16_lossy(
                    &new_text[composing.start as usize..composing.end as usize],
                );
                let marked_range = match &old.value.composing {
                    Some(old_composing) => {
                        start + old_composing.start as usize..start + old_composing.end as usize
                    }
                    None => start + prefix..start + old_end,
                };
                let selection = value.selection.as_ref().map(|selection| {
                    let base = composing.start as usize;
                    selection.base_offset as usize - base..selection.extent_offset as usize - base
                });
                handler.replace_and_mark_text_in_range(Some(marked_range), &marked, selection);
            }
            (Some(_), None) => {
                handler.replace_text_in_range(Some(replaced), &inserted);
                handler.unmark_text();
            }
            (None, None) if old_text != new_text => {
                handler.replace_text_in_range(Some(replaced), &inserted);
            }
            (None, None) => {}
        }
        self.copy = Some(Copy { start, value });
        Vec::new()
    }

    /// The input method ended editing: what it composed stays as it is.
    pub(crate) fn closed(&mut self, handler: Option<&mut PlatformInputHandler>) {
        if let Some(handler) = handler {
            handler.unmark_text();
        }
        self.started = false;
        self.copy = None;
        self.scratch = None;
    }

    fn stop(&mut self) {
        if self.started {
            self.platform.clear_client();
            self.platform.hide();
            self.started = false;
            self.copy = None;
            self.scratch = None;
            self.caret = None;
        }
    }
}

/// What `configuration` asks, as the view's input method takes it. An input whose Enter is a new
/// line is a multiline one, as Flutter pairs them; one whose Enter is an action is a line of text.
fn platform_configuration(configuration: &gpui::TextInputConfiguration) -> TextInputConfiguration {
    let input_action = platform_action(configuration.input_action);
    let input_type = match input_action {
        PlatformAction::Newline => TextInputType::Multiline,
        _ => TextInputType::Text,
    };
    TextInputConfiguration {
        input_type,
        input_action,
        obscure_text: false,
        autocorrect: configuration.autocorrect,
        enable_suggestions: configuration.suggestions,
        text_capitalization: match configuration.autocapitalize {
            Autocapitalize::None => TextCapitalization::None,
            Autocapitalize::Words => TextCapitalization::Words,
            Autocapitalize::Sentences => TextCapitalization::Sentences,
            Autocapitalize::Characters => TextCapitalization::Characters,
        },
        read_only: false,
    }
}

/// The action GPUI has the Enter key advertise, as the view's input method names it: a new line
/// where GPUI names none, since GPUI's inputs take Enter as a key.
fn platform_action(action: gpui::TextInputAction) -> PlatformAction {
    match action {
        gpui::TextInputAction::Unspecified | gpui::TextInputAction::Enter => {
            PlatformAction::Newline
        }
        gpui::TextInputAction::Done => PlatformAction::Done,
        gpui::TextInputAction::Go => PlatformAction::Go,
        gpui::TextInputAction::Next => PlatformAction::Next,
        gpui::TextInputAction::Previous => PlatformAction::Previous,
        gpui::TextInputAction::Search => PlatformAction::Search,
        gpui::TextInputAction::Send => PlatformAction::Send,
    }
}

/// The window of `handler`'s document around its selection, as the input method's value.
fn copy_of(handler: &mut PlatformInputHandler) -> Option<Copy> {
    let UTF16Selection { range, reversed } = handler.selected_text_range(false)?;
    let marked = handler.marked_text_range();
    let wanted = range.start.saturating_sub(CONTEXT_UNITS)..range.end + CONTEXT_UNITS;
    let mut adjusted = None;
    let text = handler.text_for_range(wanted.clone(), &mut adjusted)?;
    let start = adjusted.unwrap_or(wanted).start;
    let local = |offset: usize| offset.saturating_sub(start) as u32;
    let (base, extent) = if reversed {
        (range.end, range.start)
    } else {
        (range.start, range.end)
    };
    Some(Copy {
        start,
        value: TextEditingValue {
            text,
            selection: Some(TextSelection {
                base_offset: local(base),
                extent_offset: local(extent),
                affinity: TextAffinity::Downstream,
                is_directional: false,
            }),
            composing: marked.map(|marked: Range<usize>| TextRange {
                start: local(marked.start),
                end: local(marked.end),
            }),
        },
    })
}

/// Whether `handler`'s input has a document the input method can edit a copy of: a terminal has
/// none.
fn has_document(handler: &mut PlatformInputHandler) -> bool {
    handler.text_for_range(0..0, &mut None).is_some()
}

/// The range `value` composes, where it composes anything, within its text.
fn composing_of(value: &TextEditingValue) -> Option<Range<usize>> {
    let length = value.text.encode_utf16().count();
    let range = value.composing.as_ref()?;
    let (start, end) = (range.start as usize, range.end as usize);
    (start < end && end <= length).then_some(start..end)
}

fn caret_at(offset: u32) -> TextSelection {
    TextSelection {
        base_offset: offset,
        extent_offset: offset,
        affinity: TextAffinity::Downstream,
        is_directional: false,
    }
}

/// Where the caret is, at `caret` in the document, in the view's physical pixels, for the input
/// method's candidates.
fn caret_rect(handler: &mut PlatformInputHandler, caret: usize, scale: f32) -> Option<PixelRect> {
    let bounds: Bounds<Pixels> = handler.bounds_for_range(caret..caret)?;
    Some(PixelRect {
        x: f64::from(f32::from(bounds.origin.x) * scale),
        y: f64::from(f32::from(bounds.origin.y) * scale),
        width: f64::from(f32::from(bounds.size.width).max(1.0) * scale),
        height: f64::from(f32::from(bounds.size.height) * scale),
    })
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Where `new` differs from `old`: the length of their common start, and where the changed span
/// ends in each. The common start goes no further than `caret`, the start of the selection the
/// input method edited at: typing "//" before "//!" is an insertion at the caret, not after the
/// two slashes the texts happen to share, and GPUI's editors insert an empty range at the caret.
fn changed_span(old: &[u16], new: &[u16], caret: usize) -> (usize, usize, usize) {
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(old, new)| old == new)
        .count()
        .min(caret);
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count();
    (prefix, old.len() - suffix, new.len() - suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The input method's buffer holding `text`, composing `composing` of it.
    fn buffer(text: &str, composing: Option<Range<u32>>) -> TextEditingValue {
        TextEditingValue {
            selection: Some(caret_at(text.encode_utf16().count() as u32)),
            composing: composing.map(|range| TextRange {
                start: range.start,
                end: range.end,
            }),
            text: text.to_owned(),
        }
    }

    fn text(text: &str) -> Typing {
        Typing::Text(text.to_owned())
    }

    #[test]
    fn what_an_input_asks_of_the_input_method_is_passed_on() {
        let asked_of_none = platform_configuration(&gpui::TextInputConfiguration::default());
        assert!(!asked_of_none.autocorrect && !asked_of_none.enable_suggestions);
        assert!(matches!(asked_of_none.input_type, TextInputType::Multiline));
        assert!(matches!(
            asked_of_none.input_action,
            PlatformAction::Newline
        ));

        let search = platform_configuration(&gpui::TextInputConfiguration {
            autocorrect: true,
            autocapitalize: Autocapitalize::Sentences,
            suggestions: true,
            input_action: gpui::TextInputAction::Search,
        });
        assert!(search.autocorrect && search.enable_suggestions);
        assert!(matches!(
            search.text_capitalization,
            TextCapitalization::Sentences
        ));
        assert!(
            matches!(search.input_type, TextInputType::Text),
            "an Enter that is an action is a line of text's"
        );
        assert!(matches!(search.input_action, PlatformAction::Search));
    }

    #[test]
    fn letters_typed_are_typed_and_a_new_line_is_enter() {
        let mut scratch = Scratch::empty();
        assert_eq!(scratch.edit(buffer("l", None)), [text("l")]);
        assert_eq!(scratch.edit(buffer("ls", None)), [text("s")]);
        assert_eq!(
            scratch.edit(buffer("ls\npwd", None)),
            [Typing::Key(ENTER), text("pwd")]
        );
    }

    #[test]
    fn a_composition_is_shown_then_typed_once_finished() {
        let mut scratch = Scratch::empty();
        assert_eq!(
            scratch.edit(buffer("ni", Some(0..2))),
            [Typing::Preview("ni".to_owned())]
        );
        assert_eq!(
            scratch.edit(buffer("nihao", Some(0..5))),
            [Typing::Preview("nihao".to_owned())]
        );
        assert_eq!(
            scratch.edit(buffer("你好", None)),
            [text("你好"), Typing::EndPreview]
        );
    }

    #[test]
    fn part_of_a_composition_finished_is_typed_while_the_rest_is_shown() {
        let mut scratch = Scratch::empty();
        scratch.edit(buffer("nihao", Some(0..5)));
        assert_eq!(
            scratch.edit(buffer("你hao", Some(1..4))),
            [text("你"), Typing::Preview("hao".to_owned())]
        );
    }

    #[test]
    fn finished_text_deleted_is_backspace_for_each_character() {
        let mut scratch = Scratch::empty();
        scratch.edit(buffer("cd 你好", None));
        assert_eq!(
            scratch.edit(buffer("cd ", None)),
            [Typing::Key(BACKSPACE), Typing::Key(BACKSPACE)]
        );
    }

    #[test]
    fn a_korean_syllable_recomposed_is_backspace_then_the_syllable() {
        let mut scratch = Scratch::empty();
        scratch.edit(buffer("하", None));
        assert_eq!(
            scratch.edit(buffer("한", None)),
            [Typing::Key(BACKSPACE), text("한")]
        );
    }

    #[test]
    fn the_buffer_is_emptied_once_long_and_nothing_is_composed() {
        let mut scratch = Scratch::empty();
        scratch.edit(buffer(&"x".repeat(SCRATCH_UNITS), None));
        assert!(!scratch.owes_emptying());
        scratch.edit(buffer(&"x".repeat(SCRATCH_UNITS + 1), Some(0..1)));
        assert!(!scratch.owes_emptying(), "not while composing");
        scratch.edit(buffer(&"x".repeat(SCRATCH_UNITS + 1), None));
        assert!(scratch.owes_emptying());
    }
}
