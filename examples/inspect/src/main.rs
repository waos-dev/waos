//! A program with the inspector role, which it asks for by calling `wasdk::inspect`: it says this
//! device, each program on it with its icon's size and how many days of its summary are kept,
//! then traces what the first running one writes, which may be itself, and says the first chunk
//! it hears before it returns. What the engine's tests run an inspector as.
//!
//! ```text
//! cargo build -p inspect --target wasm32-wasip3
//! ```

use wasdk::inspect::{audit, devices, trace};

#[wasdk::main]
async fn main() {
    let devices = devices::devices();
    let Some(device) = devices.first() else {
        println!("no device");
        return;
    };
    println!("device {} this={}", device.name(), device.is_this_device());
    let programs = device.programs().await;
    for program in &programs {
        let icon = device.icon(program.id.clone()).await;
        let days = audit::days(device, program.id.clone()).await;
        println!(
            "program {} {:?} running={} icon={} days={}",
            program.id,
            program.name,
            program.running,
            icon.map_or(0, |icon| icon.len()),
            days.len()
        );
    }
    let Some(first) = programs.iter().find(|program| program.running) else {
        println!("nothing runs");
        return;
    };
    let Some(mut output) = trace::output(device, &first.id) else {
        println!("{} ended", first.id);
        return;
    };
    if let Some(chunk) = output.next().await {
        let written = String::from_utf8_lossy(&chunk.bytes);
        println!("{} wrote: {}", first.id, written.trim_end());
    }
}
