"""Writes `src/keys.rs`, Flutter's logical and physical keys as `u64` constants, from Waft's transcription of Flutter's generated `keyboard_key.g.dart`.

    python3 sdk/wasdk/keys/transcribe.py ../waft/crates/waft-services/src/keyboard_key.rs

Waft's file holds the constants on two types, `LogicalKeyboardKey` and `PhysicalKeyboardKey`; a key event carries the plain numbers, so each becomes a constant of `keys::logical` or `keys::physical`, with its doc, and the logical planes' masks with theirs.
"""
import pathlib
import re
import sys

OUTPUT = pathlib.Path(__file__).resolve().parent.parent / "src" / "keys.rs"

HEADER = '''//! Flutter's names of keys, the numbers a view's `key-event` carries in `logical` and `physical`.
//!
//! [`logical`] is the key as the keyboard's layout and modifiers make it, "Q" wherever the layout
//! puts it, and holds the planes its numbers are made in; [`physical`] is the key by its place, the
//! key beside Tab whatever it says, as USB HID numbers it. A key event carries numbers, as
//! Flutter's `KeyData` does, and these name them:
//!
//! ```ignore
//! use wasdk::keys::logical;
//!
//! if event.logical == logical::ARROW_LEFT { /* ... */ }
//! ```
//!
//! Written by `keys/transcribe.py` from Waft's transcription of Flutter's
//! `packages/flutter/lib/src/services/keyboard_key.g.dart`, which Flutter generates from
//! `dev/tools/gen_keycodes`: transcribe again rather than edit. Flutter's names and docs are
//! under Flutter's BSD licence, `keys/LICENSE`.
'''

CONSTANT = re.compile(
    r"pub const (?P<name>[A-Z0-9_]+): (?P<type>u64|LogicalKeyboardKey|PhysicalKeyboardKey) =\s*"
    r"(?:(?:Logical|Physical)KeyboardKey::new\()?(?P<value>0x[0-9a-f]+)\)?;"
)
FLUTTER_SEE_ALSO = re.compile(r"See the function `RawKeyEvent\.\w+` for more information\.")


def main():
    source = pathlib.Path(sys.argv[1]).read_text()
    logical_start = source.index("impl LogicalKeyboardKey {")
    physical_start = source.index("pub struct PhysicalKeyboardKey {")
    logical = constants(source[logical_start:physical_start], ("u64", "LogicalKeyboardKey"))
    physical = constants(source[physical_start:], ("PhysicalKeyboardKey",))
    OUTPUT.write_text(
        HEADER
        + module("logical", "Logical keys, and the planes their numbers are made in.", logical)
        + module("physical", "Physical keys, by their USB HID usage.", physical)
    )
    print(f"wrote {len(logical)} logical and {len(physical)} physical constants to {OUTPUT}")


def constants(section, types):
    """Each `pub const` of the given types in `section`, with the doc lines above it."""
    found = []
    doc = []
    lines = iter(section.splitlines())
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("///"):
            doc.append(stripped[3:].removeprefix(" "))
            continue
        if stripped.startswith("pub const ") and not stripped.endswith(";"):
            stripped += " " + next(lines).strip()
        match = CONSTANT.fullmatch(stripped)
        if match and match["type"] in types:
            found.append((match["name"], match["value"], trimmed(doc)))
        doc = []
    return found


def trimmed(doc):
    """The doc without Flutter's pointer to `RawKeyEvent`, which a program does not have."""
    kept = [line for line in doc if not FLUTTER_SEE_ALSO.fullmatch(line)]
    while kept and not kept[-1]:
        kept.pop()
    return kept


def module(name, doc, constants):
    body = "".join(constant(*entry) for entry in constants)
    return f"\n/// {doc}\npub mod {name} {{{body}}}\n"


def constant(name, value, doc):
    lines = "".join(f"    ///{' ' + line if line else ''}\n" for line in doc)
    return f"\n{lines}    pub const {name}: u64 = {value};\n"


if __name__ == "__main__":
    main()
