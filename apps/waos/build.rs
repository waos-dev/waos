//! Names the form factor once, as Tauri's build script does: `desktop` where WaOS's windows are
//! the system's own, `mobile` on a phone's systems, Android and iOS.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let form_factor = if os == "android" || os == "ios" {
        "mobile"
    } else {
        "desktop"
    };
    println!("cargo::rustc-cfg={form_factor}");
}
