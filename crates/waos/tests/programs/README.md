# waos-test-programs

The apps the engine's tests run, and the code folder each is compiled into, named by its content. The crate's build script builds them, so `cargo test --workspace` needs no manual step first. The engine's tests and the WaOS app's tests use it: a test loads a program from its code folder, so each program is compiled once per build of it.
