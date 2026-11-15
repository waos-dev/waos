# files

A program that uses its files through `std::fs`, and what a reader learns WaOS's files from. It writes, reads, lists and moves a file in its own `/data` and a writable mount, appending to it there; lists the folders above its mounts, `root list: cache, data, mnt, tmp`; reads a read-only mount and a symlink inside it, and is refused writing, opening for writing and removing there, `docs write: ReadOnlyFilesystem`; is refused leaving a mount by `..`, above the root, and by a symlink, relative or absolute, `escape by ..: PermissionDenied`; finds nothing outside its mounts, `outside read: NotFound`; is refused changing the folders above its mounts; and waits for `/later`, which its user mounts while it runs, reads it, waits for the user to take it away, and reads on through the file it opened before. It prints each result as `what: result`, an error by its kind.

It expects the mounts its test gives it (`crates/waos/tests/all/files.rs`): its own `/data` and `/cache`; `/mnt/docs`, read-only, holding `readme.txt`, `link-in` to it, `link-out` to `../outside.txt` and `link-abs` to that file's absolute path; `/mnt/work`, writable; and `/later`, holding `hello.txt`, added and taken away while it runs.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip3/debug/files.wasm`, from `cargo build -p files --target wasm32-wasip3`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`.
