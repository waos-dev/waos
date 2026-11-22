//! The numbers a key event names a key by, for the keys the simulated keyboard has: each
//! physical key by its USB HID usage, and each logical key by Flutter's `LogicalKeyboardKey` id, as
//! the WIT's `key-event` carries them and Waft's `waft_services` names them, which these are taken
//! from. A letter's and a function key's neighbours follow the first by one each, as in both
//! tables.

/// Physical keys: USB HID usages, page 7.
pub(super) mod physical {
    pub(in crate::simulated_user) const KEY_A: u64 = 0x0007_0004;
    pub(in crate::simulated_user) const DIGIT1: u64 = 0x0007_001e;
    pub(in crate::simulated_user) const DIGIT2: u64 = 0x0007_001f;
    pub(in crate::simulated_user) const DIGIT3: u64 = 0x0007_0020;
    pub(in crate::simulated_user) const DIGIT4: u64 = 0x0007_0021;
    pub(in crate::simulated_user) const DIGIT5: u64 = 0x0007_0022;
    pub(in crate::simulated_user) const DIGIT6: u64 = 0x0007_0023;
    pub(in crate::simulated_user) const DIGIT7: u64 = 0x0007_0024;
    pub(in crate::simulated_user) const DIGIT8: u64 = 0x0007_0025;
    pub(in crate::simulated_user) const DIGIT9: u64 = 0x0007_0026;
    pub(in crate::simulated_user) const DIGIT0: u64 = 0x0007_0027;
    pub(in crate::simulated_user) const ENTER: u64 = 0x0007_0028;
    pub(in crate::simulated_user) const ESCAPE: u64 = 0x0007_0029;
    pub(in crate::simulated_user) const BACKSPACE: u64 = 0x0007_002a;
    pub(in crate::simulated_user) const TAB: u64 = 0x0007_002b;
    pub(in crate::simulated_user) const SPACE: u64 = 0x0007_002c;
    pub(in crate::simulated_user) const MINUS: u64 = 0x0007_002d;
    pub(in crate::simulated_user) const EQUAL: u64 = 0x0007_002e;
    pub(in crate::simulated_user) const BRACKET_LEFT: u64 = 0x0007_002f;
    pub(in crate::simulated_user) const BRACKET_RIGHT: u64 = 0x0007_0030;
    pub(in crate::simulated_user) const BACKSLASH: u64 = 0x0007_0031;
    pub(in crate::simulated_user) const SEMICOLON: u64 = 0x0007_0033;
    pub(in crate::simulated_user) const QUOTE: u64 = 0x0007_0034;
    pub(in crate::simulated_user) const BACKQUOTE: u64 = 0x0007_0035;
    pub(in crate::simulated_user) const COMMA: u64 = 0x0007_0036;
    pub(in crate::simulated_user) const PERIOD: u64 = 0x0007_0037;
    pub(in crate::simulated_user) const SLASH: u64 = 0x0007_0038;
    pub(in crate::simulated_user) const F1: u64 = 0x0007_003a;
    pub(in crate::simulated_user) const HOME: u64 = 0x0007_004a;
    pub(in crate::simulated_user) const PAGE_UP: u64 = 0x0007_004b;
    pub(in crate::simulated_user) const DELETE: u64 = 0x0007_004c;
    pub(in crate::simulated_user) const END: u64 = 0x0007_004d;
    pub(in crate::simulated_user) const PAGE_DOWN: u64 = 0x0007_004e;
    pub(in crate::simulated_user) const ARROW_RIGHT: u64 = 0x0007_004f;
    pub(in crate::simulated_user) const ARROW_LEFT: u64 = 0x0007_0050;
    pub(in crate::simulated_user) const ARROW_DOWN: u64 = 0x0007_0051;
    pub(in crate::simulated_user) const ARROW_UP: u64 = 0x0007_0052;
    pub(in crate::simulated_user) const CONTROL_LEFT: u64 = 0x0007_00e0;
    pub(in crate::simulated_user) const SHIFT_LEFT: u64 = 0x0007_00e1;
    pub(in crate::simulated_user) const ALT_LEFT: u64 = 0x0007_00e2;
    pub(in crate::simulated_user) const META_LEFT: u64 = 0x0007_00e3;
}

/// Logical keys: Flutter's ids.
pub(super) mod logical {
    pub(in crate::simulated_user) const SPACE: u64 = 0x0020;
    pub(in crate::simulated_user) const KEY_A: u64 = 0x0061;
    pub(in crate::simulated_user) const BACKSPACE: u64 = 0x0001_0000_0008;
    pub(in crate::simulated_user) const TAB: u64 = 0x0001_0000_0009;
    pub(in crate::simulated_user) const ENTER: u64 = 0x0001_0000_000d;
    pub(in crate::simulated_user) const ESCAPE: u64 = 0x0001_0000_001b;
    pub(in crate::simulated_user) const DELETE: u64 = 0x0001_0000_007f;
    pub(in crate::simulated_user) const ARROW_DOWN: u64 = 0x0001_0000_0301;
    pub(in crate::simulated_user) const ARROW_LEFT: u64 = 0x0001_0000_0302;
    pub(in crate::simulated_user) const ARROW_RIGHT: u64 = 0x0001_0000_0303;
    pub(in crate::simulated_user) const ARROW_UP: u64 = 0x0001_0000_0304;
    pub(in crate::simulated_user) const END: u64 = 0x0001_0000_0305;
    pub(in crate::simulated_user) const HOME: u64 = 0x0001_0000_0306;
    pub(in crate::simulated_user) const PAGE_DOWN: u64 = 0x0001_0000_0307;
    pub(in crate::simulated_user) const PAGE_UP: u64 = 0x0001_0000_0308;
    pub(in crate::simulated_user) const F1: u64 = 0x0001_0000_0801;
    pub(in crate::simulated_user) const CONTROL_LEFT: u64 = 0x0002_0000_0100;
    pub(in crate::simulated_user) const SHIFT_LEFT: u64 = 0x0002_0000_0102;
    pub(in crate::simulated_user) const ALT_LEFT: u64 = 0x0002_0000_0104;
    pub(in crate::simulated_user) const META_LEFT: u64 = 0x0002_0000_0106;
}
