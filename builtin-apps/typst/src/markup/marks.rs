//! What each stretch of the markup is, as Typst's own parser and highlighter tell it: the text
//! parsed whole, then cut into runs, each with the tag of the innermost syntax node that has one.
//!
//! The invariant a later editor will break: the runs cover the text from its start to its end
//! with no gap and no overlap, in order, so the editor's spans add up to exactly its text.

use std::ops::Range;

use typst::syntax::{LinkedNode, Tag, highlight, parse};

/// A stretch of the text, in bytes, and what it is; none where it is plain text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) bytes: Range<usize>,
    pub(crate) tag: Option<Tag>,
}

/// The runs of `text`.
pub(crate) fn runs_of(text: &str) -> Vec<Run> {
    let root = parse(text);
    let mut runs = Vec::new();
    collect(&LinkedNode::new(&root), None, &mut runs);
    cover(runs, text.len())
}

/// Adds the runs of `node`'s leaves, each tagged as itself or, failing that, as `inherited`.
fn collect(node: &LinkedNode, inherited: Option<Tag>, runs: &mut Vec<Run>) {
    let tag = highlight(node).or(inherited);
    if node.children().len() == 0 {
        push(runs, node.range(), tag);
        return;
    }
    for child in node.children() {
        collect(&child, tag, runs);
    }
}

/// Adds a run, joined to the last where they touch and are the same.
fn push(runs: &mut Vec<Run>, bytes: Range<usize>, tag: Option<Tag>) {
    if bytes.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.tag == tag && last.bytes.end == bytes.start => {
            last.bytes.end = bytes.end;
        }
        _ => runs.push(Run { bytes, tag }),
    }
}

/// `runs` with plain runs wherever they leave some of the `length` bytes out.
fn cover(runs: Vec<Run>, length: usize) -> Vec<Run> {
    let mut covering = Vec::with_capacity(runs.len());
    let mut covered = 0;
    for run in runs {
        if run.bytes.start < covered || run.bytes.end > length {
            continue;
        }
        push(&mut covering, covered..run.bytes.start, None);
        covered = run.bytes.end;
        push(&mut covering, run.bytes, run.tag);
    }
    push(&mut covering, covered..length, None);
    covering
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(text: &str) -> Vec<(&str, Option<Tag>)> {
        runs_of(text)
            .into_iter()
            .map(|run| (&text[run.bytes], run.tag))
            .collect()
    }

    #[test]
    fn the_runs_cover_the_text_in_order() {
        let text = "= Title\nSome *bold* text, $x^2$ and #lorem(3). // done\n";
        let runs = runs_of(text);
        assert_eq!(runs.first().map(|run| run.bytes.start), Some(0));
        assert_eq!(runs.last().map(|run| run.bytes.end), Some(text.len()));
        for pair in runs.windows(2) {
            assert_eq!(pair[0].bytes.end, pair[1].bytes.start);
        }
    }

    #[test]
    fn a_heading_strong_text_and_a_comment_are_told_apart() {
        let tagged = tags("= Title\n*bold* // note");
        assert!(tagged.contains(&("= Title", Some(Tag::Heading))));
        assert!(tagged.contains(&("*bold*", Some(Tag::Strong))));
        assert!(tagged.contains(&("// note", Some(Tag::Comment))));
    }

    #[test]
    fn empty_text_has_no_runs() {
        assert!(runs_of("").is_empty());
    }
}
