//! Waft's video decoders the programs opened, and each of the engine's calls carried out on them:
//! a decoder opened on the decoders of the GPU the call names as the engine asks, kept once it
//! opened until the program lets it go, each chunk handed to it and each frame asked of it, and
//! each answer sent back once Waft has it. Waft's decoders live on one thread, so this does too:
//! the main thread where WaOS is a Waft app (`state::video_decoders`), a thread of the decoders'
//! own where it is not (`video_decoder_thread`).
//!
//! The invariant: every call is answered once, with what Waft answered, or with a failure where
//! the decoder is not open here; and a decoder opened for a program that no longer waits for it
//! is let go at once, so none is kept that no program holds.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{Either, LocalBoxFuture, select};
use waft::ui::GpuId;
use waft::ui::video_decoders as decoders;

use super::video_decoder_calls::{DecoderCall, DecoderKey, no_support};

/// Starts a task on the thread Waft's decoders live on: what waits for one of their answers.
pub(crate) type StartTask<'a> = &'a mut dyn FnMut(LocalBoxFuture<'static, ()>);

/// Waft's decoders of the GPU an id names, the system's default where none; none where it has
/// none.
pub(crate) type GpuDecoders<'a> =
    &'a dyn Fn(Option<GpuId>) -> Option<Rc<dyn decoders::VideoDecoders>>;

/// Waft's decoders the programs opened, by the app's key for each.
#[derive(Clone, Default)]
pub(crate) struct OpenVideoDecoders(Rc<RefCell<HashMap<DecoderKey, OpenDecoder>>>);

/// One of Waft's decoders a program opened, with what ends the watch for its end as it goes.
struct OpenDecoder {
    decoder: Arc<dyn decoders::VideoDecoder>,
    _removed: oneshot::Sender<()>,
}

impl OpenVideoDecoders {
    /// Carries out `call`, from the engine, on Waft's decoders of the GPU it names, which
    /// `decoders_of` answers, each wait for an answer a task `start_task` starts.
    pub(crate) fn take(
        &self,
        decoders_of: GpuDecoders<'_>,
        start_task: StartTask<'_>,
        call: DecoderCall,
    ) {
        match call {
            DecoderCall::Support {
                gpu,
                description,
                answer,
            } => {
                let support =
                    decoders_of(gpu).map_or_else(no_support, |system| system.support(&description));
                // A program that ended meanwhile hears nothing.
                let _ = answer.send(support);
            }
            DecoderCall::Open {
                gpu,
                decoder,
                setup,
                options,
                answer,
                ended,
            } => match decoders_of(gpu) {
                Some(system) => {
                    let opening = system.open(setup, options);
                    start_task(self.kept_once_opened(decoder, opening, answer, ended));
                }
                None => drop(answer.send(Err(no_decoders()))),
            },
            DecoderCall::Decode {
                decoder,
                chunk,
                answer,
            } => match self.opened(decoder) {
                Some(waft) => start_task(answered_once_done(waft.decode(chunk), answer)),
                None => drop(answer.send(Err(not_open()))),
            },
            DecoderCall::NextFrame { decoder, answer } => match self.opened(decoder) {
                Some(waft) => start_task(answered_once_done(waft.next_frame(), answer)),
                None => drop(answer.send(None)),
            },
            DecoderCall::Drain { decoder, answer } => match self.opened(decoder) {
                Some(waft) => start_task(answered_once_done(waft.drain(), answer)),
                None => drop(answer.send(Err(not_open()))),
            },
            DecoderCall::Reset(decoder) => {
                if let Some(waft) = self.opened(decoder) {
                    waft.reset();
                }
            }
            DecoderCall::Configure {
                decoder,
                setup,
                answer,
            } => match self.opened(decoder) {
                Some(waft) => start_task(answered_once_done(waft.configure(setup), answer)),
                None => drop(answer.send(Err(not_open()))),
            },
            DecoderCall::Remove(decoder) => {
                self.0.borrow_mut().remove(&decoder);
            }
        }
    }

    /// Waft's decoder open under `decoder`, if it is.
    fn opened(&self, decoder: DecoderKey) -> Option<Arc<dyn decoders::VideoDecoder>> {
        self.0
            .borrow()
            .get(&decoder)
            .map(|open| Arc::clone(&open.decoder))
    }

    /// Answers into `answer` once Waft has opened the decoder `opening` opens, keeping it under
    /// `decoder` while a program waits for it, and tells `ended` why, should it end by itself
    /// before the program lets it go.
    fn kept_once_opened(
        &self,
        decoder: DecoderKey,
        opening: decoders::VideoDecoderOpenFuture,
        answer: oneshot::Sender<Result<decoders::CodecKind, decoders::DecoderError>>,
        ended: oneshot::Sender<decoders::DecoderError>,
    ) -> LocalBoxFuture<'static, ()> {
        let open = self.clone();
        async move {
            let waft = match opening.await {
                Ok(waft) => waft,
                // A program that ended meanwhile hears nothing.
                Err(error) => return drop(answer.send(Err(error))),
            };
            let (removed, was_removed) = oneshot::channel();
            let has_ended = waft.ended();
            let kind = waft.kind();
            open.0.borrow_mut().insert(
                decoder,
                OpenDecoder {
                    decoder: waft,
                    _removed: removed,
                },
            );
            if answer.send(Ok(kind)).is_err() {
                open.0.borrow_mut().remove(&decoder);
                return;
            }
            if let Either::Left((reason, _)) = select(has_ended, was_removed).await {
                let _ = ended.send(reason);
            }
        }
        .boxed_local()
    }
}

/// Answers into `answer` what `doing` answers, once Waft has.
fn answered_once_done<T: 'static>(
    doing: impl Future<Output = T> + 'static,
    answer: oneshot::Sender<T>,
) -> LocalBoxFuture<'static, ()> {
    async move {
        // A program that ended meanwhile hears nothing.
        let _ = answer.send(doing.await);
    }
    .boxed_local()
}

fn not_open() -> decoders::DecoderError {
    decoders::DecoderError::Failed("the decoder is not open in WaOS".to_owned())
}

/// What opening a decoder of a GPU with none is answered.
fn no_decoders() -> decoders::DecoderError {
    decoders::DecoderError::Unsupported("the GPU has no video decoders".to_owned())
}
