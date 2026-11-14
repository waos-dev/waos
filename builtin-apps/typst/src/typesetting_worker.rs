//! The typesetting worker, as the window reaches it: started with the app, sent the window's
//! requests in the order it makes them, and each reply handed to what it answers, the pages
//! ([`pages`](crate::pages)), a page's picture ([`page_images`](crate::page_images)) or a PDF
//! ([`exporting`](crate::exporting)).
//!
//! A request is queued at once, whatever the worker is doing, so the window never waits for it:
//! one made before the worker has started waits for it, and the worker answers each in turn.

use futures::StreamExt;
use futures::channel::mpsc;
use waft::App;
use waft_winui::InfoBarSeverity;
use wasdk::workers::{self, Worker, WorkerError};
use wasdk::{StreamReader, StreamWriter, wit_stream};

use crate::documents_app::DocumentsApp;
use crate::exporting::pdf_made;
use crate::notices::say;
use crate::page_images::page_drawn;
use crate::pages::pages_set;
use crate::typesetting::messages::{Reply, Request, decoded, encoded};
use crate::typesetting::worker::TYPESETTER_ENTRY;

/// Where the window's requests to the worker queue. Every clone is the same queue.
#[derive(Clone, Debug)]
pub(crate) struct TypesettingWorker {
    requests: mpsc::UnboundedSender<Request>,
}

/// The requests queued, until the worker takes them.
pub(crate) struct QueuedRequests(mpsc::UnboundedReceiver<Request>);

impl TypesettingWorker {
    /// A queue for the worker that [`start_typesetting`] starts.
    pub(crate) fn new() -> (TypesettingWorker, QueuedRequests) {
        let (requests, queued) = mpsc::unbounded();
        (TypesettingWorker { requests }, QueuedRequests(queued))
    }

    /// Asks the worker for `request`; its reply comes in turn.
    pub(crate) fn ask(&self, request: Request) {
        // A worker gone, which a note said, answers nothing more.
        let _ = self.requests.unbounded_send(request);
    }
}

/// Starts the typesetting worker, hands it the requests `queued` and those to come, and hands
/// each of its replies on; says why where it does not start, or once it has failed.
pub(crate) fn start_typesetting(app: &mut App, documents: &DocumentsApp, queued: QueuedRequests) {
    let documents = documents.clone();
    app.spawn(async move |cx| {
        let (to_worker, inbox) = wit_stream::new();
        let (worker, mut replies) = match started(inbox).await {
            Ok(started) => started,
            Err(why) => {
                cx.update(|app| say(app, &documents, InfoBarSeverity::Error, why));
                return;
            }
        };
        let handing_on = async {
            while let Some(reply) = replies.next().await {
                if let Some(reply) = decoded(&reply) {
                    cx.update(|app| answered(app, &documents, reply));
                }
            }
            if let Err(why) = worker.finished().await {
                let said = format!("Typesetting stopped: {}", why.lines().next().unwrap_or(""));
                cx.update(|app| say(app, &documents, InfoBarSeverity::Error, said));
            }
        };
        futures::future::join(send(queued, to_worker), handing_on).await;
    });
}

/// The worker started on `inbox`, and what it replies.
async fn started(inbox: StreamReader<Vec<u8>>) -> Result<(Worker, StreamReader<Vec<u8>>), String> {
    let Some(workers) = workers::get_workers() else {
        return Err("Documents cannot set pages here: this system runs no workers.".to_owned());
    };
    let worker = workers
        .start(TYPESETTER_ENTRY.to_owned(), inbox)
        .await
        .map_err(|error| match error {
            WorkerError::UnknownEntry(entry) => format!("Documents has no worker “{entry}”."),
            WorkerError::Refused(why) => format!("Typesetting could not start: {why}."),
        })?;
    let replies = worker
        .messages()
        .expect("a worker's messages are there once");
    Ok((worker, replies))
}

/// Writes each request queued to the worker, in order, until the worker or the queue is gone.
async fn send(queued: QueuedRequests, mut to_worker: StreamWriter<Vec<u8>>) {
    let QueuedRequests(mut queued) = queued;
    while let Some(request) = queued.next().await {
        if to_worker.write_one(encoded(&request)).await.is_some() {
            return;
        }
    }
}

/// Hands `reply` to what it answers.
fn answered(app: &mut App, documents: &DocumentsApp, reply: Reply) {
    match reply {
        Reply::Set(set) => pages_set(app, documents, set),
        Reply::Drawn(drawn) => page_drawn(app, documents, drawn),
        Reply::Pdf(pdf) => pdf_made(app, documents, pdf),
    }
}
