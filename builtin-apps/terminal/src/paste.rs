//! Pasted text as a terminal types it: each line ended as Enter ends one, and marked as pasted
//! where the command asked, so a shell does not run a pasted line before the user has seen it.

/// What marks the start and the end of pasted text for a command that asked (bracketed paste).
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// The bytes that paste `text`, `bracketed` where the command asked for pasted text to be marked.
/// Escape is left out of what is pasted: text that held the end mark would otherwise end the
/// paste early, and what followed would be typed as commands.
pub(crate) fn pasted_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let typed = text
        .replace("\r\n", "\r")
        .replace('\n', "\r")
        .replace('\x1b', "");
    if !bracketed {
        return typed.into_bytes();
    }
    [PASTE_START, typed.as_bytes(), PASTE_END].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_end_as_enter_ends_them() {
        assert_eq!(pasted_bytes("a\nb\r\nc", false), b"a\rb\rc");
    }

    #[test]
    fn a_bracketed_paste_is_marked() {
        assert_eq!(pasted_bytes("ls", true), b"\x1b[200~ls\x1b[201~");
    }

    #[test]
    fn pasted_text_cannot_end_the_paste_itself() {
        let pasted = pasted_bytes("a\x1b[201~rm -rf", true);
        assert_eq!(pasted, b"\x1b[200~a[201~rm -rf\x1b[201~");
    }
}
