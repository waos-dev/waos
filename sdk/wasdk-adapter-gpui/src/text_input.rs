//! A window's text input: GPUI's `NSTextInputClient`-shaped input handler bridged to the view's
//! Flutter-shaped `text-input`.
//!
//! GPUI's platforms ask the input handler whenever they need the text; WaOS's input methods read
//! a copy the program pushes instead, and send each edit back as the whole new value. So after
//! each frame this pushes a window of the document around the selection, and turns each value
//! that comes back into GPUI's edits, from its difference with the value pushed. Offsets are UTF-16
//! on both sides.
//!
//! The invariant: an edit is read against the value last pushed or received, never the document
//! as it is now, since the input method edited that copy; so every value received becomes the
//! copy before the next push.

use std::ops::Range;

use gpui::{Bounds, Pixels, PlatformInputHandler, UTF16Selection};
use wasdk::views::{
    PixelRect, TextAffinity, TextCapitalization, TextEditingValue, TextInput,
    TextInputAction as PlatformAction, TextInputConfiguration, TextInputType, TextRange,
    TextSelection,
};

/// How many UTF-16 units of the document either side of the selection the input method sees:
/// the document's length is not known, as Android's `getTextBeforeCursor` does not know it.
const CONTEXT_UNITS: usize = 1000;

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

pub(crate) struct TextInputBridge {
    platform: TextInput,
    started: bool,
    copy: Option<Copy>,
    /// The caret's rectangle pushed last, as x, y, width and height.
    caret: Option<(f64, f64, f64, f64)>,
}

impl TextInputBridge {
    pub(crate) fn new(platform: TextInput) -> TextInputBridge {
        TextInputBridge {
            platform,
            started: false,
            copy: None,
            caret: None,
        }
    }

    /// Brings the input method up to date with `handler`, the focused input or none, at `scale`
    /// physical pixels to a logical one.
    pub(crate) fn sync(&mut self, handler: Option<&mut PlatformInputHandler>, scale: f32) {
        let Some(handler) = handler else {
            self.stop();
            return;
        };
        if !handler.query_accepts_text_input() {
            self.stop();
            return;
        }
        if !self.started {
            self.platform.start(configuration());
            self.platform.show();
            self.started = true;
        }
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
        if let Some(caret) = caret_rect(handler, &copy, scale) {
            let place = (caret.x, caret.y, caret.width, caret.height);
            if self.caret != Some(place) {
                self.platform.set_caret_rect(caret);
                self.caret = Some(place);
            }
        }
    }

    /// Applies the input method's `value` to `handler`, as GPUI's edits.
    pub(crate) fn apply(&mut self, value: TextEditingValue, handler: &mut PlatformInputHandler) {
        let Some(old) = self.copy.take() else {
            return;
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
    }

    /// The input method ended editing: what it composed stays as it is.
    pub(crate) fn closed(&mut self, handler: Option<&mut PlatformInputHandler>) {
        if let Some(handler) = handler {
            handler.unmark_text();
        }
        self.started = false;
        self.copy = None;
    }

    fn stop(&mut self) {
        if self.started {
            self.platform.stop();
            self.started = false;
            self.copy = None;
            self.caret = None;
        }
    }
}

fn configuration() -> TextInputConfiguration {
    TextInputConfiguration {
        input_type: TextInputType::Multiline,
        input_action: PlatformAction::Newline,
        obscure_text: false,
        autocorrect: false,
        enable_suggestions: false,
        text_capitalization: TextCapitalization::None,
        read_only: false,
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

/// Where the caret is, in the view's physical pixels, for the input method's candidates.
fn caret_rect(handler: &mut PlatformInputHandler, copy: &Copy, scale: f32) -> Option<PixelRect> {
    let selection = copy.value.selection.as_ref()?;
    let caret = copy.start + selection.extent_offset as usize;
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
