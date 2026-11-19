//! A program that floods stdout while a background timer increments `/data/ticks`, for
//! testing async output backpressure: while nobody reads stdout and writes wait, the timer task
//! keeps running and a stop ends the program at once. With `--println`, floods through `println!`
//! (standard output's synchronous write).
//!
//! ```text
//! cargo build -p flood --target wasm32-wasip3
//! ```

use std::time::Duration;

use wasdk::monotonic_clock;

mod stdout_async {
    wit_bindgen::generate!({
        path: "../../wit",
        generate_all,
        inline: "
            package example:flood;

            world flood {
                import wasi:cli/stdout@0.3.0;
            }
        ",
    });
}

#[wasdk::main]
async fn main() {
    let chunk_bytes: usize = 64 * 1024;
    if std::env::args().nth(1).as_deref() == Some("--println") {
        let line = "x".repeat(chunk_bytes - 1);
        let mut wrote = 0u64;
        loop {
            println!("{line}");
            wrote += chunk_bytes as u64;
            let _ = std::fs::write("/data/wrote", wrote.to_string());
        }
    }

    wasdk::spawn(async {
        let mut ticks: u64 = 0;
        loop {
            ticks += 1;
            let _ = std::fs::write("/data/ticks", ticks.to_string());
            monotonic_clock::wait_for(Duration::from_millis(10).as_nanos() as u64).await;
        }
    });

    let (mut writer, reader) = stdout_async::wit_stream::new::<u8>();
    wasdk::spawn(async move {
        let _ = stdout_async::wasi::cli::stdout::write_via_stream(reader).await;
    });

    let chunk = vec![b'x'; chunk_bytes];
    let mut wrote = 0u64;
    loop {
        let remaining = writer.write_all(chunk.clone()).await;
        if !remaining.is_empty() {
            break;
        }
        wrote += chunk_bytes as u64;
        let _ = std::fs::write("/data/wrote", wrote.to_string());
        monotonic_clock::wait_for(Duration::from_millis(1).as_nanos() as u64).await;
    }
}
