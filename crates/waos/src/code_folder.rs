//! A component's code folder: the code the engine compiles from one component, kept in a folder
//! the caller gives beside it, and loaded by mapping its file rather than reading it. The code's
//! pages are then the file's, clean, which the system drops and reads again as it likes, where
//! code read from a file is decompressed into the process's own memory: 4 MiB of footprint
//! against 104 MiB, and a load of 4-9 ms against 57 ms (measured on gallery).
//!
//! The invariants a later editor will break:
//! - A code folder belongs to one component that never changes. Nothing here checks the
//!   component against its code: a component changed in place would map the code of what it was.
//! - Each file is named by the engine that compiled it, from its `precompile_compatibility_hash`,
//!   so an engine of another wasmtime or other settings finds no file of its own and compiles. The
//!   name carries native unwind information too, which wasmtime maps whether the engine asked for
//!   it or not.
//! - A file is never changed where it is, since a running program may map it: it is written whole
//!   beside its name and renamed to it. Other engines' files are removed when this one's is
//!   written; one a program maps (Windows refuses to remove it) stays, never mapped again, and
//!   goes at a later compile or with the folder.
//! - Only this module writes the files it maps. wasmtime trusts what it maps, as it trusts its
//!   own cache directory: a file anyone else can write is a way into the process.

use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use wasmtime::Engine;
use wasmtime::component::Component;

/// What a code file's name ends with.
const EXTENSION: &str = "cwasm";

/// Whether `code` holds the code `engine` compiles.
pub(crate) fn is_compiled(engine: &Engine, code: &Path) -> bool {
    file_of(engine, code).is_file()
}

/// Compiles the component at `component` with `engine` into `code`, and removes the code other
/// engines compiled there.
pub(crate) fn compile(engine: &Engine, component: &Path, code: &Path) -> Result<()> {
    tracing::info!(
        component = %component.display(),
        "compiling the component into {}",
        code.display()
    );
    let bytes = fs::read(component).with_context(|| format!("reading {}", component.display()))?;
    let compiled = engine
        .precompile_component(&bytes)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("compiling {}", component.display()))?;
    fs::create_dir_all(code).with_context(|| format!("making {}", code.display()))?;
    let file = file_of(engine, code);
    write_whole(&file, &compiled).with_context(|| format!("writing {}", file.display()))?;
    remove_others(code, &file);
    Ok(())
}

/// The component at `component`, mapped from `engine`'s code in `code`, compiled there first
/// when there is none.
pub(crate) fn load(engine: &Engine, component: &Path, code: &Path) -> Result<Component> {
    if !is_compiled(engine, code) {
        compile(engine, component, code)?;
    }
    map(engine, &file_of(engine, code))
}

/// The file `engine`'s code is kept in, in `code`.
fn file_of(engine: &Engine, code: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    engine.precompile_compatibility_hash().hash(&mut hasher);
    code.join(format!("{:016x}.{EXTENSION}", hasher.finish()))
}

/// Removes every code file of `code` but `kept`; one a program maps stays, said so.
fn remove_others(code: &Path, kept: &Path) {
    let Ok(entries) = fs::read_dir(code) else {
        return;
    };
    for other in entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
    {
        if other != kept
            && other
                .extension()
                .is_some_and(|extension| extension == EXTENSION)
            && let Err(error) = fs::remove_file(&other)
        {
            tracing::info!(file = %other.display(), %error, "another engine's code stays");
        }
    }
}

/// Maps the code compiled into `compiled`; wasmtime refuses it when it was compiled by another
/// wasmtime or with other settings it checks.
// Mapping code compiled earlier is an unsafe API of wasmtime's, wrapped here; the rest of the
// crate has no `unsafe`.
#[allow(unsafe_code)]
fn map(engine: &Engine, compiled: &Path) -> Result<Component> {
    // SAFETY: the file is one `compile` wrote, into a folder only this module writes, and it is
    // never changed where it is, only removed, so the pages mapped stay as they were for as long
    // as the component lives.
    let component = unsafe { Component::deserialize_file(engine, compiled) };
    component
        .map_err(anyhow::Error::from)
        .with_context(|| format!("mapping {}", compiled.display()))
}

/// Writes `contents` into a file beside `path` of this writer's own, then renames it to `path`:
/// the folder holds the file only once it is whole. Where another writer put it there meanwhile,
/// the same code, that one stays.
fn write_whole(path: &Path, contents: &[u8]) -> Result<()> {
    static WRITTEN: AtomicU64 = AtomicU64::new(0);
    let written = WRITTEN.fetch_add(1, Ordering::Relaxed);
    let mut partial = path.as_os_str().to_owned();
    partial.push(format!(".{}-{written}.partial", std::process::id()));
    let partial = PathBuf::from(partial);
    let renamed = write_synced(&partial, contents)
        .and_then(|()| fs::rename(&partial, path))
        .context("writing the file whole");
    if renamed.is_err() {
        let _ = fs::remove_file(&partial);
        if path.is_file() {
            return Ok(());
        }
    }
    renamed
}

fn write_synced(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(contents)?;
    file.sync_all()
}
