//! What a program writes, as the shell writes it: each item in its forms, the content of each
//! taken out of the program's store as it is written (a stream being read, a file by its path on
//! the machine), then each stream read to its end and each promise, a form or a file, produced by
//! the program's source when someone pastes it.

use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::oneshot;
use futures::future::join_all;

use super::sources::{PromisedForm, Source};
use crate::shell::{Content, CopiedItem};

/// One item as the program wrote it, each form keyed by its MIME type.
pub(crate) struct WrittenItem(pub(crate) Vec<(String, WrittenForm)>);

/// One form's content, taken out of the program's store.
pub(crate) enum WrittenForm {
    /// The bytes of a stream, once it has ended.
    Data(oneshot::Receiver<Vec<u8>>),
    Promise,
    /// A file, by its path on the machine.
    File(PathBuf),
    /// A file that exists only once pasted, by its name.
    FilePromise(String),
}

/// The forms `items` promise, files aside, each by its item's place and its type: what a flush
/// or a stop asks the program for.
pub(crate) fn promised_forms(items: &[WrittenItem]) -> Vec<PromisedForm> {
    let mut promised = Vec::new();
    for (item, WrittenItem(forms)) in (0u32..).zip(items) {
        for (mime, form) in forms {
            if matches!(form, WrittenForm::Promise) {
                promised.push(PromisedForm {
                    item,
                    mime: mime.clone(),
                });
            }
        }
    }
    promised
}

/// `items` as the shell writes them, each stream read to its end and each promise produced by
/// `source`.
pub(crate) async fn copied(items: Vec<WrittenItem>, source: &Source) -> Vec<CopiedItem> {
    let copied = (0u32..)
        .zip(items)
        .map(|(item, WrittenItem(forms))| async move {
            let representations = forms.into_iter().map(|(mime, form)| async move {
                let content = content_of(
                    form,
                    PromisedForm {
                        item,
                        mime: mime.clone(),
                    },
                    source,
                )
                .await;
                (mime, content)
            });
            CopiedItem {
                representations: join_all(representations).await,
            }
        });
    join_all(copied).await
}

/// One form's content as the shell writes it.
async fn content_of(form: WrittenForm, promised: PromisedForm, source: &Source) -> Content {
    match form {
        // A stream the program dropped unended gave what it wrote before.
        WrittenForm::Data(bytes) => Content::Data(bytes.await.unwrap_or_default()),
        WrittenForm::Promise => Content::Promise(Arc::new(source.producer(promised))),
        WrittenForm::File(path) => Content::File(path),
        WrittenForm::FilePromise(name) => Content::FilePromise {
            name,
            produce: Arc::new(source.file_producer(promised)),
        },
    }
}
