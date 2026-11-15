//! The clipboard's text, as GPUI reads and writes it.
//!
//! GPUI's write is fire and forget: the write goes on in a task of GPUI's. GPUI's read, and Zed's 128 call
//! sites of it (at Zed 703b5ac3f4), are synchronous, while wasdk's is asynchronous; so the read waits for the
//! platform's answer with `wasdk::block_on`, the SDK's way for a library that cannot await, which
//! holds the program's one thread for the read.

use futures::StreamExt;
use gpui::{ClipboardItem, ForegroundExecutor};
use wasdk::clipboard::{Clipboard, Content, CopyOptions};
use wasdk::wit_stream;

const TEXT: &str = "text/plain";

/// The first item's text, read now.
pub(crate) fn read_blocking(clipboard: &Clipboard) -> Option<ClipboardItem> {
    let item = clipboard
        .items()
        .into_iter()
        .find(|item| item.types().iter().any(|kind| kind.starts_with(TEXT)))?;
    let started = std::time::Instant::now();
    let bytes = wasdk::block_on(async move {
        let stream = item
            .read(TEXT.to_owned())
            .await
            .inspect_err(|error| log::warn!("clipboard read failed: {error:?}"))
            .ok()?;
        Some(stream.into_stream().collect::<Vec<u8>>().await)
    })?;
    log::debug!(
        "clipboard read of {} bytes took {:?}",
        bytes.len(),
        started.elapsed()
    );
    Some(ClipboardItem::new_string(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

/// Puts `text` on the clipboard, in a task of `executor`'s.
pub(crate) fn write(executor: &ForegroundExecutor, clipboard: Clipboard, text: String) {
    executor
        .spawn(async move {
            let (mut writer, content) = wit_stream::new::<u8>();
            let item = vec![(TEXT.to_owned(), Content::Data(content))];
            let written = clipboard.write(vec![item], CopyOptions::new());
            let bytes = text.into_bytes();
            let (result, _) = futures::join!(written, async move { writer.write_all(bytes).await });
            if let Err(error) = result {
                log::warn!("clipboard write failed: {error:?}");
            }
        })
        .detach();
}
