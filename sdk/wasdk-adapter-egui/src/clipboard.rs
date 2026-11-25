//! Copy, cut and paste between egui and the platform's clipboard.
//!
//! The user's commands come as a view's events, each with what carries it out: a copy or a cut
//! with the request that writes, a paste with what was on the clipboard. egui answers a copy one
//! pass later, with the text it copied among that pass's commands, so the request waits here
//! until then. A copy egui makes with no request behind it, as a button that copies does, goes
//! through the program's clipboard, under its write switch.

use wasdk::clipboard::{self, Content, CopyOptions, CopyRequest, Paste};
use wasdk::wit_stream;

const PLAIN_TEXT: &str = "text/plain";

/// The user's copy or cut that egui has not answered yet.
#[derive(Default)]
pub(super) struct PendingCopy(Option<CopyRequest>);

impl PendingCopy {
    pub(super) fn wait_with(&mut self, request: CopyRequest) {
        self.0 = Some(request);
    }

    /// Writes `text` as the user's copy when one waits, or else as a copy of the program's own.
    pub(super) fn write(&mut self, text: String) {
        let request = self.0.take();
        wasdk::spawn(async move {
            let items = vec![vec![(
                PLAIN_TEXT.to_owned(),
                Content::Data(stream_of(text)),
            )]];
            let written = match request {
                Some(request) => request.write(items, CopyOptions::new()).await,
                None => match clipboard::get_clipboard() {
                    Some(clipboard) => clipboard.write(items, CopyOptions::new()).await,
                    None => return,
                },
            };
            if let Err(error) = written {
                tracing::warn!("the copy was not written: {error}");
            }
        });
    }

    /// Lets go of a request egui had nothing to copy for.
    pub(super) fn forget(&mut self) {
        self.0 = None;
    }
}

fn stream_of(text: String) -> wasdk::StreamReader<u8> {
    let (mut writer, reader) = wit_stream::new::<u8>();
    wasdk::spawn(async move {
        writer.write_all(text.into_bytes()).await;
    });
    reader
}

/// The text of what the user pasted; none when no item of it has text.
pub(super) async fn pasted_text(paste: Paste) -> Option<String> {
    for item in paste.items() {
        if !item.types().iter().any(|mime| mime == PLAIN_TEXT) {
            continue;
        }
        if let Ok(content) = item.read(PLAIN_TEXT.to_owned()).await {
            return Some(String::from_utf8_lossy(&content.collect().await).into_owned());
        }
    }
    None
}
