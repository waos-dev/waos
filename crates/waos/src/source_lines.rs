//! Where in its source a program failed: the frames of a trap's backtrace, each named by its
//! function and, where the program was built with line tables, by its file and line, read from
//! the DWARF of the component's own file once the program has failed, and only then.
//!
//! The engine compiles programs without wasmtime's backtrace details: with them, wasmtime turns
//! every frame into a file and line on each unwind, a stop's among them, parsing the program's
//! DWARF again for every frame, which took a debug build of `examples/camera`, 166 MB, up to 21 s
//! to stop (`../experiments/2026-11-16-waos-stop-backtrace`). wasmtime keeps no DWARF without
//! them, so the component's file is read again here, with the same `addr2line` and `wasmparser`
//! wasmtime reads it with. A frame's offset is the component's own, as wasmtime translates a
//! component's modules where they lie in it, so the frame's module is the one whose bytes hold
//! it.
//!
//! The invariant: a frame is located only from the file the program was loaded from, as it was
//! then; a file changed since leaves the backtrace as wasmtime made it, never located in code it
//! did not run.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use addr2line::gimli::{self, EndianSlice, LittleEndian};
use wasmparser::{Parser, Payload};
use wasmtime::WasmBacktrace;

/// The file a program was loaded from, as it was then.
#[derive(Clone, Debug)]
pub(crate) struct ComponentFile {
    path: PathBuf,
    /// When it was last changed, as the system said at the load; none where it could not say.
    modified: Option<SystemTime>,
}

/// A failed program's backtrace, located in its source, over the failure beneath it.
struct Located {
    frames: Vec<Frame>,
    /// The failure, its outermost layer the backtrace this one replaces.
    failure: anyhow::Error,
}

/// One frame of a backtrace: where in the component it was, its function, and what the
/// program's DWARF says of it, the innermost of the functions inlined there first.
struct Frame {
    offset: Option<usize>,
    /// Its module, by the name wasmtime gives it.
    module: String,
    function: String,
    symbols: Vec<Symbol>,
}

/// A function the DWARF names at a frame, and where in the source it was.
struct Symbol {
    function: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
}

/// A core module of the component that carries DWARF: where its bytes are in the component,
/// where its code section's contents begin, which the DWARF's addresses count from, and its
/// DWARF sections by name.
struct DwarfModule<'a> {
    range: Range<usize>,
    code_start: u64,
    sections: HashMap<&'a str, &'a [u8]>,
}

impl ComponentFile {
    /// The file at `path`, as it is now.
    pub(crate) fn at(path: &Path) -> ComponentFile {
        ComponentFile {
            path: path.to_owned(),
            modified: fs::metadata(path).and_then(|file| file.modified()).ok(),
        }
    }

    /// Its contents, while it is the file the program was loaded from.
    fn read_unchanged(&self) -> Option<Vec<u8>> {
        let modified = fs::metadata(&self.path)
            .and_then(|file| file.modified())
            .ok();
        if modified.is_none() || modified != self.modified {
            return None;
        }
        fs::read(&self.path).ok()
    }
}

/// `failure`, a program's, its backtrace located in the source of the program loaded from
/// `file`; as it is where it has no backtrace, or no frame of it can be located, a program built
/// without line tables among them. A backtrace is
/// the outermost layer of the failure it is on: wasmtime adds it last, and the engine adds none
/// before the program's end is made.
pub(crate) fn located(failure: anyhow::Error, file: &ComponentFile) -> anyhow::Error {
    let Some(backtrace) = failure.downcast_ref::<WasmBacktrace>() else {
        return failure;
    };
    let Some(component) = file.read_unchanged() else {
        return failure;
    };
    let frames = located_frames(backtrace, &component);
    if frames.iter().all(|frame| frame.symbols.is_empty()) {
        return failure;
    }
    anyhow::Error::new(Located { frames, failure })
}

/// Each of `backtrace`'s frames, located in the DWARF of the module of `component` it is in.
fn located_frames(backtrace: &WasmBacktrace, component: &[u8]) -> Vec<Frame> {
    let modules = dwarf_modules(component);
    let mut contexts: HashMap<usize, Option<addr2line::Context<EndianSlice<'_, LittleEndian>>>> =
        HashMap::new();
    backtrace
        .frames()
        .iter()
        .map(|frame| {
            let offset = frame.module_offset();
            let symbols = offset
                .and_then(|offset| {
                    let index = modules
                        .iter()
                        .position(|module| module.range.contains(&offset))?;
                    let context = contexts
                        .entry(index)
                        .or_insert_with(|| modules[index].context())
                        .as_ref()?;
                    Some(symbols_at(
                        context,
                        offset as u64 - modules[index].code_start,
                    ))
                })
                .unwrap_or_default();
            Frame {
                offset,
                module: frame.module().name().unwrap_or("<unknown>").to_owned(),
                function: function_name(frame.func_name(), frame.func_index()),
                symbols,
            }
        })
        .collect()
}

/// The core modules of `component` that carry DWARF, nested components' among them, each where
/// it lies in `component`.
fn dwarf_modules(component: &[u8]) -> Vec<DwarfModule<'_>> {
    let mut modules = Vec::new();
    // What each payload is part of: a module being read, by its index in `modules`, or not.
    let mut within: Vec<Option<usize>> = Vec::new();
    for payload in Parser::new(0).parse_all(component) {
        let Ok(payload) = payload else {
            break;
        };
        match payload {
            Payload::ModuleSection {
                unchecked_range, ..
            } => {
                modules.push(DwarfModule {
                    range: unchecked_range.start as usize..unchecked_range.end as usize,
                    code_start: 0,
                    sections: HashMap::new(),
                });
                within.push(Some(modules.len() - 1));
            }
            Payload::ComponentSection { .. } => within.push(None),
            Payload::End(_) => {
                within.pop();
            }
            Payload::CodeSectionStart { range, .. } => {
                if let Some(Some(index)) = within.last() {
                    modules[*index].code_start = range.start;
                }
            }
            Payload::CustomSection(section) if section.name().starts_with(".debug_") => {
                if let Some(Some(index)) = within.last() {
                    modules[*index]
                        .sections
                        .insert(section.name(), section.data());
                }
            }
            _ => {}
        }
    }
    modules.retain(|module| !module.sections.is_empty());
    modules
}

impl<'a> DwarfModule<'a> {
    /// What finds the source of an address in this module's code; none where its DWARF cannot
    /// be read.
    fn context(&self) -> Option<addr2line::Context<EndianSlice<'a, LittleEndian>>> {
        let dwarf = gimli::Dwarf::load(|id| {
            let data = self.sections.get(id.name()).copied().unwrap_or(&[]);
            Ok::<_, gimli::Error>(EndianSlice::new(data, LittleEndian))
        })
        .ok()?;
        addr2line::Context::from_dwarf(dwarf).ok()
    }
}

/// What `context` says of the address `address` of its code: each function there, the
/// innermost first, with its file, line and column.
fn symbols_at(
    context: &addr2line::Context<EndianSlice<'_, LittleEndian>>,
    address: u64,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let Ok(mut frames) = context.find_frames(address).skip_all_loads() else {
        return symbols;
    };
    while let Ok(Some(frame)) = frames.next() {
        let location = frame.location.as_ref();
        symbols.push(Symbol {
            function: frame
                .function
                .as_ref()
                .and_then(|function| function.raw_name().ok())
                .map(|name| demangled(&name)),
            file: location
                .and_then(|location| location.file)
                .map(str::to_owned),
            line: location.and_then(|location| location.line),
            column: location.and_then(|location| location.column),
        });
    }
    symbols
}

/// A frame's function as wasmtime names it: demangled, or by its index where it has no name.
fn function_name(name: Option<&str>, index: u32) -> String {
    match name {
        Some(name) => demangled(name),
        None => format!("<wasm function {index}>"),
    }
}

fn demangled(name: &str) -> String {
    match rustc_demangle::try_demangle(name) {
        Ok(demangled) => demangled.to_string(),
        Err(_) => name.to_owned(),
    }
}

/// As wasmtime writes a backtrace with its details: each frame's offset and function, then each
/// function the DWARF names there with its file and line.
impl fmt::Display for Located {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error while executing at wasm backtrace:")?;
        for (index, frame) in self.frames.iter().enumerate() {
            write!(f, "\n  {index:>3}: ")?;
            if let Some(offset) = frame.offset {
                write!(f, "{offset:#8x} - ")?;
            }
            if frame.symbols.is_empty() {
                write!(f, "{}!{}", frame.module, frame.function)?;
            }
            for (inlined, symbol) in frame.symbols.iter().enumerate() {
                if inlined > 0 {
                    write!(f, "\n                - ")?;
                }
                match &symbol.function {
                    Some(function) => write!(f, "{function}")?,
                    None if inlined == 0 => write!(f, "{}", frame.function)?,
                    None => write!(f, "<inlined function>")?,
                }
                if let Some(file) = &symbol.file {
                    write!(f, "\n                    at {file}")?;
                    if let Some(line) = symbol.line {
                        write!(f, ":{line}")?;
                        if let Some(column) = symbol.column {
                            write!(f, ":{column}")?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Located {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Located {
    /// The failure beneath the backtrace.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.failure.chain().nth(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_changed_since_the_load_is_not_read() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("program.wasm");
        fs::write(&path, b"before").unwrap();
        let file = ComponentFile::at(&path);
        assert_eq!(file.read_unchanged().as_deref(), Some(&b"before"[..]));
        let later = SystemTime::now() + std::time::Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(file.read_unchanged(), None, "built again since");
    }
}
