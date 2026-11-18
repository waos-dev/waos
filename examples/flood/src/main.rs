//! A program that floods stdout while a background timer increments `/data/ticks`, for
//! testing async output backpressure: while nobody reads stdout and writes wait, the timer task
//! keeps running and a stop ends the program at once.
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

    let chunk = vec![b'x'; 64 * 1024];
    loop {
        let remaining = writer.write_all(chunk.clone()).await;
        if !remaining.is_empty() {
            break;
        }
        monotonic_clock::wait_for(Duration::from_millis(1).as_nanos() as u64).await;
    }
}
