//! A Waft app that draws a picture from its own bytes, over `wasdk-adapter-waft`: a red PNG,
//! given to Waft's `Image::memory` and stretched over its whole window. It writes no decoding of
//! its own: Waft's `MemoryImage` asks the platform for a codec, `instantiateImageCodec`, which the
//! adapter answers through the system's image codecs.
//!
//! ```text
//! cargo build -p waft-image --target wasm32-wasip3
//! waos run -p waft-image
//! ```

use waft_embedder::WindowConfig;
use waft_widgets::{BoxFit, Image, IntoWidget, run_app_with};

/// A PNG of four by four red pixels.
const RED_PNG: &[u8] = include_bytes!("red.png");

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        let window = WindowConfig {
            title: "Waft image".to_owned(),
            size: [320.0, 240.0],
            ..WindowConfig::default()
        };
        let picture = Image::memory_static(RED_PNG).fit(BoxFit::Fill);
        run_app_with(app, picture.into_widget(), window);
    })
    .await;
}
