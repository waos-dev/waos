//! The size of the terminal's text, which every window shares and View's commands change.

use waft_foundation::{App, Entity};

use crate::font::monospace_family;
use crate::grid_painter::TextSettings;

/// The size the text starts at and Default Size returns to, in logical pixels.
const DEFAULT_SIZE: f64 = 13.0;
const SMALLEST_SIZE: f64 = 8.0;
const LARGEST_SIZE: f64 = 36.0;

/// One of View's commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextSizeStep {
    Bigger,
    Smaller,
    Default,
}

/// The text every window starts with: the platform's monospace font at the default size.
pub(crate) fn first_text_settings() -> TextSettings {
    TextSettings {
        family: monospace_family(&wasdk::fonts::families()),
        size: DEFAULT_SIZE,
    }
}

/// The size `step` takes text of `size` to.
fn stepped(size: f64, step: TextSizeStep) -> f64 {
    match step {
        TextSizeStep::Bigger => (size + 1.0).min(LARGEST_SIZE),
        TextSizeStep::Smaller => (size - 1.0).max(SMALLEST_SIZE),
        TextSizeStep::Default => DEFAULT_SIZE,
    }
}

/// Sizes the text of `text` by `step`, in every window.
pub(crate) fn step_text_size(app: &mut App, text: &Entity<TextSettings>, step: TextSizeStep) {
    text.update(app, |text, cx| {
        let size = stepped(text.size, step);
        if size != text.size {
            text.size = size;
            cx.notify();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_steps_by_one_within_its_bounds() {
        assert_eq!(stepped(13.0, TextSizeStep::Bigger), 14.0);
        assert_eq!(stepped(13.0, TextSizeStep::Smaller), 12.0);
        assert_eq!(stepped(36.0, TextSizeStep::Bigger), 36.0);
        assert_eq!(stepped(8.0, TextSizeStep::Smaller), 8.0);
        assert_eq!(stepped(20.0, TextSizeStep::Default), 13.0);
    }
}
