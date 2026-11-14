# fonts

A program that reads the fonts it was given, as a text stack would: it counts the installed families, finds the face covering a letter, that face's family and the bold among it, and the system font; reads the system font's character map from the `cmap` table the engine serves to find the letter's glyph; opens the system font's whole file, reads it into a buffer of the length the engine gives, and finds the same glyph in it; and asks the engine for that glyph's coverage and outline, printing what came back. A program given no fonts prints that there are none: no family, no face, and no glyph of a font it names anyway.

- **Takes:** `wasdk`; `read-fonts`, which reads the character map.
- **Provides:** a component, `target/wasm32-wasip2/debug/fonts.wasm`, from `cargo build -p fonts --target wasm32-wasip2`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`, given this computer's fonts and given none.
