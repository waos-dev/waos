//! Programs compiled ahead of time into files of the engine's, and loaded by mapping that file
//! rather than reading it: the code's pages are the file's, clean, which the system drops and
//! reads again as it likes, where code read from a file is decompressed into the process's own
//! memory. The experiment measured it on gallery: 4 MiB of footprint against 104 MiB, and a load
//! of 4-9 ms against 57 ms (measured on gallery).
//!
//! The invariants a later editor will break:
//! - A file is never changed where it is, since a running program may have it mapped. Each compile
//!   writes a file of its own under a name no other file had, written whole beside it and renamed
//!   to it, so that a file of the cache is always whole; then the component's files compiled from
//!   it as it was before go. Two compiles of the same component at once each leave a file, both
//!   its code.
//!   A program mapping one keeps its pages: Unix's systems remove a mapped file's name and keep
//!   its pages, and Windows, which refuses to remove a file wasmtime maps (it shares the file for
//!   reading alone), lets its date change, so a file the system will not remove is dated
//!   [`RETIRED`], never mapped again, and removed by a later compile or removal once nothing maps
//!   it.
//! - A file is mapped only while it is compiled from its component as the component is now,
//!   which the file's modification time says, set to the component's when it was compiled; and
//!   by an engine of the same wasmtime and settings as the one that compiled it, which the file's
//!   name says, carrying the engine's `precompile_compatibility_hash`. wasmtime checks the
//!   settings again at the load, but not native unwind information, which it maps whether the
//!   engine asked for it or not. Anything else is compiled again, never trusted.
//! - Only this module writes the files it maps. wasmtime trusts what it maps, as it trusts its
//!   own cache directory: a file anyone else can write is a way into the process.
//! - A component's files are found by its component's path, which must still be there: their
//!   names begin with a hash of the path made whole, which cannot be made of a path that is gone.
//!   So the code of a component about to be removed is removed first, and a file written for a
//!   component removed while it compiled is removed once written.

use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::{Context, Result};
use wasmtime::Engine;
use wasmtime::component::Component;

/// The date of a file that is no component's code any longer but which the system would not
/// remove, because a program maps it: no component is dated so, so it is never mapped again.
const RETIRED: SystemTime = SystemTime::UNIX_EPOCH;

const GIB: u64 = 1024 * 1024 * 1024;
/// Upper bound on threads used to compile one component, so high-core machines do not multiply
/// per-thread Cranelift allocator arenas without bound.
const MAX_COMPILE_THREADS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SystemMemory {
    total_bytes: u64,
    available_bytes: u64,
}

/// The component at `component`, mapped from its file in `cache`, which it is compiled into
/// first when there is none compiled from it as it is now by an engine like `engine`.
pub(crate) fn load(engine: &Engine, component: &Path, cache: &Path) -> Result<Component> {
    let files = ComponentFiles::of(cache, engine, component)?;
    if let Some(current) = files.current(component) {
        return map(engine, &current);
    }
    tracing::info!(
        component = %component.display(),
        "compiling the component into {}",
        cache.display()
    );
    let compiled = compile_into(engine, component, &files)?;
    map(engine, &compiled)
}

/// Compiles the component at `component` into a file in `cache`, as [`load`] would, unless one
/// holds its code already.
pub(crate) fn compile(engine: &Engine, component: &Path, cache: &Path) -> Result<()> {
    let files = ComponentFiles::of(cache, engine, component)?;
    if files.current(component).is_some() {
        return Ok(());
    }
    tracing::info!(
        component = %component.display(),
        "compiling the component ahead of its load into {}",
        cache.display()
    );
    compile_into(engine, component, &files).map(drop)
}

/// Whether a file in `cache` holds the code `engine` compiles from the component at `component`
/// as it is now, so that [`load`] maps it without compiling.
pub(crate) fn is_compiled(engine: &Engine, component: &Path, cache: &Path) -> Result<bool> {
    let files = ComponentFiles::of(cache, engine, component)?;
    Ok(files.current(component).is_some())
}

/// Removes the code `engine` compiled from the component at `component` from `cache`, if there
/// is any; the component must still be there. A program mapping it keeps its pages.
pub(crate) fn remove(engine: &Engine, component: &Path, cache: &Path) -> Result<()> {
    let files = ComponentFiles::of(cache, engine, component)?;
    for file in files.all() {
        retire(&file)?;
    }
    remove_retired(cache);
    Ok(())
}

/// The files in a cache of the code an engine compiles from one component: each named after the
/// component's file and its whole path, so two programs with one file name do not share one, and
/// after the engine's settings, so code compiled with others is never mapped; then after the
/// compile that wrote it, so a compile never writes over a file a program may map.
struct ComponentFiles {
    cache: PathBuf,
    prefix: String,
}

impl ComponentFiles {
    fn of(cache: &Path, engine: &Engine, component: &Path) -> Result<ComponentFiles> {
        let whole = fs::canonicalize(component)
            .with_context(|| format!("finding {}", component.display()))?;
        let mut hasher = DefaultHasher::new();
        whole.hash(&mut hasher);
        engine.precompile_compatibility_hash().hash(&mut hasher);
        let stem = whole.file_stem().unwrap_or_default().to_string_lossy();
        Ok(ComponentFiles {
            cache: cache.to_path_buf(),
            prefix: format!("{stem}-{:016x}-", hasher.finish()),
        })
    }

    /// Every file of the component's code there is, current or not.
    fn all(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.cache) else {
            return Vec::new();
        };
        entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| self.holds(path))
            .collect()
    }

    /// The file compiled from `component` as it is now, if there is one.
    fn current(&self, component: &Path) -> Option<PathBuf> {
        let now = modified(component).ok()?;
        self.all()
            .into_iter()
            .find(|file| modified(file).is_ok_and(|compiled| compiled == now))
    }

    /// A name for a new file of the component's code, which no other writer, in this process or
    /// another, uses.
    fn new_file(&self) -> PathBuf {
        static WRITTEN: AtomicU64 = AtomicU64::new(0);
        let written = WRITTEN.fetch_add(1, Ordering::Relaxed);
        self.cache.join(format!(
            "{}{}-{written}.cwasm",
            self.prefix,
            std::process::id()
        ))
    }

    fn holds(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&self.prefix) && name.ends_with(".cwasm"))
    }
}

/// Compiles the component at `component` with `engine` into a new file of `files`, and retires
/// the component's files compiled from it as it was before; answers the new one. The file
/// carries the component's modification time.
fn compile_into(engine: &Engine, component: &Path, files: &ComponentFiles) -> Result<PathBuf> {
    // Read before the bytes, so that a component changed while it compiles reads as changed.
    let dated = modified(component)?;
    let bytes = read_component(component)?;
    let code = precompile(engine, component, &bytes)?;
    drop(bytes);
    let compiled = files.new_file();
    write_whole(&compiled, &code, dated)
        .with_context(|| format!("writing {}", compiled.display()))?;
    for earlier in files.all() {
        if modified(&earlier).is_ok_and(|date| date != dated) {
            retire(&earlier)?;
        }
    }
    if !component.exists() {
        tracing::info!(
            component = %component.display(),
            "the component was removed while it compiled: its code goes too"
        );
        retire(&compiled)?;
    }
    remove_retired(&files.cache);
    Ok(compiled)
}

/// Precompiles `bytes` on a scoped thread pool sized to available CPUs and RAM so compilation
/// leaves headroom for the UI and releases its worker threads' allocator arenas as soon as it
/// finishes.
fn precompile(engine: &Engine, component: &Path, bytes: &[u8]) -> Result<Vec<u8>> {
    let cpus = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let threads = compile_threads(cpus, system_memory());
    tracing::debug!(
        component = %component.display(),
        threads,
        bytes = bytes.len(),
        "precompiling component"
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|index| format!("waos-compile-{index}"))
        .start_handler(|_| crate::os::lower_thread_priority())
        .build()
        .context("starting the compile thread pool")?;
    let code = pool
        .install(|| engine.precompile_component(bytes))
        .map_err(anyhow::Error::from)
        .with_context(|| format!("compiling {}", component.display()));
    drop(pool);
    code
}

/// How many threads to give one component compile: leaves a core for the UI, caps at
/// [`MAX_COMPILE_THREADS`], and scales down on low-memory devices so Cranelift's per-thread
/// arenas do not push the process into the low-memory killer.
fn compile_threads(cpus: usize, memory: Option<SystemMemory>) -> usize {
    let by_cpu = cpus.saturating_sub(1).clamp(1, MAX_COMPILE_THREADS);
    let Some(memory) = memory else {
        return by_cpu;
    };
    let by_memory = if memory.available_bytes < GIB {
        1
    } else if memory.total_bytes <= 4 * GIB || memory.available_bytes < 2 * GIB {
        2
    } else if memory.total_bytes <= 8 * GIB || memory.available_bytes < 4 * GIB {
        4
    } else {
        MAX_COMPILE_THREADS
    };
    by_cpu.min(by_memory)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn system_memory() -> Option<SystemMemory> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    parse_proc_meminfo(&text)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn system_memory() -> Option<SystemMemory> {
    None
}

#[cfg(any(target_os = "linux", target_os = "android", test))]
fn parse_proc_meminfo(text: &str) -> Option<SystemMemory> {
    let mut total_kib = None;
    let mut available_kib = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kib = parse_meminfo_kib(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available_kib = parse_meminfo_kib(rest);
        }
    }
    Some(SystemMemory {
        total_bytes: total_kib?.checked_mul(1024)?,
        available_bytes: available_kib?.checked_mul(1024)?,
    })
}

#[cfg(any(target_os = "linux", target_os = "android", test))]
fn parse_meminfo_kib(value: &str) -> Option<u64> {
    value.split_whitespace().next()?.parse().ok()
}

/// Reads the component file for compilation. On Android every component compiled by WaOS lives in
/// the store's immutable unpacked version folder, so mapping avoids copying large `.wasm` files
/// into private heap; elsewhere the file is read so a development rebuild cannot truncate a mapped
/// file.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
fn read_component(component: &Path) -> Result<memmap2::Mmap> {
    let file = File::open(component).with_context(|| format!("opening {}", component.display()))?;
    // SAFETY: on Android every component compiled by WaOS lives in the store's unpacked version
    // directory, which is written whole before becoming current and never modified in place.
    unsafe { memmap2::Mmap::map(&file) }.with_context(|| format!("mapping {}", component.display()))
}

#[cfg(not(target_os = "android"))]
fn read_component(component: &Path) -> Result<Vec<u8>> {
    fs::read(component).with_context(|| format!("reading {}", component.display()))
}

/// Removes `file`, which is no component's code any longer; where the system will not, since a
/// program maps it, dates it [`RETIRED`], so it is never mapped again and goes at a later
/// [`remove_retired`].
fn retire(file: &Path) -> Result<()> {
    match fs::remove_file(file) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) if is_mapped(&error) => {
            tracing::info!(file = %file.display(), "a program maps the code: it goes once none does");
            match date_retired(file) {
                Ok(()) => Ok(()),
                Err(error) if is_going(&error) => Ok(()),
                Err(error) => Err(error).with_context(|| format!("retiring {}", file.display())),
            }
        }
        Err(error) => Err(error).with_context(|| format!("removing {}", file.display())),
    }
}

/// Whether dating a file failed because another run of the same component is removing it at
/// once: the file is gone, or, on Windows, its removal is pending, which refuses every new
/// handle, the attributes' one too.
fn is_going(error: &std::io::Error) -> bool {
    error.kind() == ErrorKind::NotFound || is_delete_pending(error)
}

#[cfg(windows)]
fn is_delete_pending(error: &std::io::Error) -> bool {
    const ERROR_ACCESS_DENIED: i32 = 5;
    error.raw_os_error() == Some(ERROR_ACCESS_DENIED)
}

#[cfg(not(windows))]
fn is_delete_pending(_error: &std::io::Error) -> bool {
    false
}

/// Removes every file of `cache` dated [`RETIRED`] that nothing maps any longer; the rest stay
/// for a later call.
fn remove_retired(cache: &Path) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let file = entry.path();
        if file
            .extension()
            .is_some_and(|extension| extension == "cwasm")
            && modified(&file).is_ok_and(|date| date == RETIRED)
        {
            let _ = fs::remove_file(&file);
        }
    }
}

/// Whether removing a file failed because a program maps it: only Windows refuses that, with a
/// sharing violation, wasmtime having opened the file sharing it for reading alone.
#[cfg(windows)]
fn is_mapped(error: &std::io::Error) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_ACCESS_DENIED: i32 = 5;
    matches!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION | ERROR_ACCESS_DENIED)
    )
}

#[cfg(not(windows))]
fn is_mapped(_error: &std::io::Error) -> bool {
    false
}

/// Dates `file` [`RETIRED`], through a handle that asks to write its attributes alone, which
/// Windows grants whatever the file's other handles share, wasmtime's among them.
#[cfg(windows)]
fn date_retired(file: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
    fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .open(file)?
        .set_modified(RETIRED)
}

#[cfg(not(windows))]
fn date_retired(file: &Path) -> std::io::Result<()> {
    File::options()
        .write(true)
        .open(file)?
        .set_modified(RETIRED)
}

/// Maps the code compiled into `compiled`; wasmtime refuses it when it was compiled by another
/// wasmtime or with other settings it checks.
// Mapping code compiled earlier is an unsafe API of wasmtime's, wrapped here; the rest of the
// crate has no `unsafe`.
#[allow(unsafe_code)]
fn map(engine: &Engine, compiled: &Path) -> Result<Component> {
    // SAFETY: the file is one `compile_into` wrote, and it is never changed where it is, only
    // removed or dated, so the pages mapped stay as they were for as long as the component lives.
    let component = unsafe { Component::deserialize_file(engine, compiled) };
    component
        .map_err(anyhow::Error::from)
        .with_context(|| format!("mapping {}", compiled.display()))
}

fn modified(path: &Path) -> Result<SystemTime> {
    let metadata = fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(metadata.modified()?)
}

/// Writes `contents`, dated `modified`, into a file beside `path`, then renames it to `path`, a
/// name no file has: the cache holds the file only once it is whole.
fn write_whole(path: &Path, contents: &[u8], modified: SystemTime) -> Result<()> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let written = write_dated(&partial, contents, modified)
        .and_then(|()| fs::rename(&partial, path))
        .context("writing the file whole");
    if written.is_err() {
        let _ = fs::remove_file(&partial);
    }
    written
}

fn write_dated(path: &Path, contents: &[u8], modified: SystemTime) -> std::io::Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(contents)?;
    file.set_modified(modified)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file opened as wasmtime opens one it maps: sharing it for reading alone on Windows.
    fn opened_as_wasmtime_maps(file: &Path) -> File {
        let mut options = File::options();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 1;
            options.share_mode(FILE_SHARE_READ);
        }
        options.open(file).unwrap()
    }

    #[test]
    fn a_file_a_program_maps_is_retired_never_current_and_goes_once_none_does() {
        let cache = tempfile::tempdir().unwrap();
        let file = cache.path().join("program-0123456789abcdef-1-0.cwasm");
        let compiled_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        write_dated(&file, b"code", compiled_at).unwrap();
        let mapped = opened_as_wasmtime_maps(&file);

        retire(&file).unwrap();
        assert_eq!(
            file.exists(),
            cfg!(windows),
            "only Windows keeps a file a program maps"
        );
        if file.exists() {
            assert_eq!(
                modified(&file).unwrap(),
                RETIRED,
                "dated never to be mapped"
            );
        }
        remove_retired(cache.path());
        drop(mapped);
        remove_retired(cache.path());
        assert!(!file.exists(), "gone once nothing maps it");
    }

    #[test]
    fn compile_threads_scales_with_cpus_and_memory() {
        assert_eq!(compile_threads(1, None), 1);
        assert_eq!(compile_threads(4, None), 3);
        assert_eq!(compile_threads(16, None), MAX_COMPILE_THREADS);

        let pixel_4gb = SystemMemory {
            total_bytes: 3_930_000_000,
            available_bytes: 1_900_000_000,
        };
        assert_eq!(compile_threads(8, Some(pixel_4gb)), 2);

        let low_available = SystemMemory {
            total_bytes: 8 * GIB,
            available_bytes: 800 * 1024 * 1024,
        };
        assert_eq!(compile_threads(8, Some(low_available)), 1);

        let mid_8gb = SystemMemory {
            total_bytes: 8 * GIB,
            available_bytes: 5 * GIB,
        };
        assert_eq!(compile_threads(8, Some(mid_8gb)), 4);

        let phone_12gb = SystemMemory {
            total_bytes: 12 * GIB,
            available_bytes: 6 * GIB,
        };
        assert_eq!(compile_threads(8, Some(phone_12gb)), 7);
    }

    #[test]
    fn proc_meminfo_parses_total_and_available_kib() {
        let sample = "\
MemTotal:        3838896 kB
MemFree:          112340 kB
MemAvailable:    1654320 kB
Buffers:           12345 kB
";
        assert_eq!(
            parse_proc_meminfo(sample),
            Some(SystemMemory {
                total_bytes: 3_838_896 * 1024,
                available_bytes: 1_654_320 * 1024,
            })
        );
        assert_eq!(parse_proc_meminfo("MemTotal: 1024 kB\n"), None);
    }
}
