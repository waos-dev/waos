//! The system's video decoders on a thread of their own: how a WaOS that is no Waft app, a
//! headless one, serves them to the engine. Waft's decoders for this system (`os`) are made on
//! the thread and stay there, as they would on a Waft app's main thread, and the engine's calls
//! reach it as they reach that one (`video_decoder_calls`), carried out the same
//! (`open_video_decoders`). Decoding needs no window, so nothing here asks for one.
//!
//! The invariant: the thread ends once the engine has let go of its shell's decoders, and what it
//! was waiting on then is dropped, which answers every call still open.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::executor::LocalPool;
use futures::task::LocalSpawnExt;
use waos::ShellVideoDecoders;

use super::open_video_decoders::OpenVideoDecoders;
use super::video_decoder_calls::{DecoderCall, DecodersOnRequest};
use crate::os;

/// The system's decoders as the engine's shell has them, each call carried out on a thread
/// started here; none where Waft has no decoders for this system.
pub(crate) fn start() -> Option<Arc<dyn ShellVideoDecoders>> {
    let (shell, arriving) = DecodersOnRequest::new();
    let (found, was_found) = mpsc::channel();
    thread::Builder::new()
        .name("waos video decoders".to_owned())
        .spawn(move || {
            // Waft's decoders are not sent between threads, so they are made on this one.
            let system = os::video_decoders();
            let _ = found.send(system.is_some());
            if let Some(system) = system {
                serve(&*system, arriving);
            }
        })
        .expect("the system starts a thread");
    let found = was_found.recv().unwrap_or(false);
    found.then(|| Arc::new(shell) as Arc<dyn ShellVideoDecoders>)
}

/// Carries out each call that arrives on `system`, until no more can.
fn serve(
    system: &dyn waft::ui::video_decoders::VideoDecoders,
    mut arriving: UnboundedReceiver<DecoderCall>,
) {
    let mut tasks = LocalPool::new();
    let spawner = tasks.spawner();
    let open = OpenVideoDecoders::default();
    tasks.run_until(async {
        while let Some(call) = arriving.next().await {
            let mut start_task = |task| {
                spawner
                    .spawn_local(task)
                    .expect("the thread's tasks run until it ends");
            };
            open.take(system, &mut start_task, call);
        }
    });
}
