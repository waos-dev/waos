//! A program with the inspector role, which it asks for by calling `wasdk::inspect`: it says this
//! device, each program on it with its icon's size and how many days of its summary are kept,
//! then the first running one's mounts, reads one mount's entries and tries a path that leaves it,
//! says how many uses it has active and traces what it writes, which may be itself, saying the
//! first chunk it hears before it returns. What the engine's tests run an inspector as.
//!
//! ```text
//! cargo build -p inspect --target wasm32-wasip3
//! ```

use wasdk::inspect::{active_uses, audit, devices, storage, trace};

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
    let mounts = storage::mounts(device, &first.id);
    for mount in &mounts {
        println!(
            "mount {} {:?} writable={} at {}",
            mount.path, mount.kind, mount.writable, mount.place
        );
    }
    if let Some(mount) = mounts.first() {
        match storage::read_folder(device, first.id.clone(), mount.path.clone()).await {
            Ok(entries) => {
                for entry in &entries {
                    println!(
                        "entry {} folder={} size={:?} modified={}",
                        entry.name,
                        entry.is_folder,
                        entry.size,
                        entry.modified.is_some()
                    );
                }
            }
            Err(why) => println!("entries of {} failed: {why}", mount.path),
        }
        let escape = format!("{}/../..", mount.path);
        match storage::read_folder(device, first.id.clone(), escape).await {
            Ok(_) => println!("escape read"),
            Err(why) => println!("escape refused: {why}"),
        }
    }
    if let Some((active, _changes)) = active_uses::active_uses(device, &first.id) {
        println!("{} uses {} now", first.id, active.len());
    }
    let Some(mut output) = trace::output(device, &first.id) else {
        println!("{} ended", first.id);
        return;
    };
    if let Some(chunk) = output.next().await {
        let written = String::from_utf8_lossy(&chunk.bytes);
        println!("{} wrote: {}", first.id, written.trim_end());
    }
}
