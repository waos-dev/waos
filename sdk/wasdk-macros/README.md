# wasdk-macros

`#[wasdk::main]` and `#[wasdk::worker]`: a program's entry and its workers' entries, which generation cannot produce. Neither takes options.

- **Takes:** a program's `async fn main`, returning `()` or a `Result<(), E>` where `E: Display`; a worker's `async fn` of its inbox, a `StreamReader<Vec<u8>>`, and its outbox, a `StreamWriter<Vec<u8>>`, returning nothing.
- **Provides:** the export of `wasi:cli/run@0.3.0`, the async entry the engine calls, with the program's error printed and reported as its failure; the export of `wasdk:platform/worker-entry`, which runs the worker registered at the entry asked on the instance's executor and answers the reading end of its outbox, an entry with no worker answered `unknown-entry`, so a program without workers writes nothing for them; the `fn main` the `wasm32-wasip2` target links a Preview 2 entry to in every `bin`, which only says it is not the entry; and for each worker, its registration under its name, a static constructor `wasm-ld` runs before the first export does anything, and `<NAME>_ENTRY`, the name as a constant beside the function, with its visibility.
- **Used by:** `wasdk`, which re-exports them; a program reaches them as `wasdk::main` and `wasdk::worker`.

```rust
#[wasdk::main]
async fn main() -> Result<(), String> {
    for view in wasdk::views::views() {
        // …
    }
    Ok(())
}
```

```rust
#[wasdk::worker]
async fn compile(mut inbox: StreamReader<Vec<u8>>, mut outbox: StreamWriter<Vec<u8>>) {
    while let Some(source) = inbox.next().await {
        outbox.write_one(render_pages(&source)).await;
    }
}

// in the main instance:
let (mut to_worker, inbox) = wit_stream::new();
let worker = workers.start(COMPILE_ENTRY.to_owned(), inbox).await?;
```

A worker's function is in the program's own crate: its registration is a static constructor in the crate that defines it, and a dependency's code the program never calls is not linked.
