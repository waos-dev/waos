//! A composition layer's presents of the program's hardware buffers: each waits, on a thread of the
//! layer's own, for the GPU work the program submitted and wrote to draw into the buffer, what it
//! wrote submitted for it as it presented (`ProgramHardwareBuffer::submitted_work`), then goes to
//! the shell's layer, which shows it in place of what the layer showed. The program goes on
//! meanwhile, and hears through the answer to its present when the shell is done with the buffer.
//! Each wait counts as the program's GPU work in flight until its hand-over is over, so the app's
//! suspending waits for it. The thread starts at the layer's first present and ends when the
//! program lets go of the layer.
//!
//! The invariants: the shell is handed the buffers in the order the program presented them, so a
//! later present never shows before an earlier one; and a buffer is held, with the frame it is the
//! memory of and the bytes the program is counted for, until the shell is done with it and no
//! screenshot reads it (`ProgramHardwareBuffer::after_reads`).

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use tokio::sync::oneshot;

use crate::gpu::{Finishing, GpuInstance};
use crate::hardware_buffers::{ProgramHardwareBuffer, SubmittedWork};
use crate::shell::ShellCompositionLayer;

/// The layer's handle on its thread.
pub(crate) struct LayerPresents {
    presents: Sender<Present>,
}

/// One present, for the thread: the buffer, the work to wait for, and where to say the shell is
/// done with it.
struct Present {
    buffer: ProgramHardwareBuffer,
    work: SubmittedWork,
    released: oneshot::Sender<()>,
    /// The program's GPU work in flight, which the present counts in until it is handed over.
    in_flight: Finishing,
}

impl LayerPresents {
    /// Starts the thread that hands the program's buffers to `layer`, the shell's, once `gpu` has
    /// drawn them.
    pub(crate) fn start(gpu: GpuInstance, layer: Arc<dyn ShellCompositionLayer>) -> LayerPresents {
        let (presents, presented) = mpsc::channel();
        thread::Builder::new()
            .name("presents of a composition layer".into())
            .spawn(move || show_each(&gpu, layer.as_ref(), &presented))
            .expect("a thread to present a composition layer's buffers on");
        LayerPresents { presents }
    }

    /// Hands `buffer` to the shell's layer once the GPU has finished `work`, what the program
    /// submitted and wrote to draw into it, after every buffer presented before it, counted in
    /// `in_flight` until then. Resolves once the shell is done with it, or as WaOS lets go of the
    /// layer unshown.
    pub(crate) fn present(
        &self,
        buffer: ProgramHardwareBuffer,
        work: SubmittedWork,
        in_flight: Finishing,
    ) -> oneshot::Receiver<()> {
        let (released, answer) = oneshot::channel();
        // The thread ends only when this sender is dropped; a present it never takes answers as
        // its sender goes.
        let _ = self.presents.send(Present {
            buffer,
            work,
            released,
            in_flight,
        });
        answer
    }
}

fn show_each(gpu: &GpuInstance, layer: &dyn ShellCompositionLayer, presents: &Receiver<Present>) {
    for present in presents {
        present.work.wait(gpu);
        let Present {
            buffer,
            released,
            in_flight,
            ..
        } = present;
        let shown = buffer.shell_buffer();
        let read = buffer.clone();
        let done = Box::new(move || {
            drop(buffer);
            let _ = released.send(());
        });
        layer.present(shown, read.after_reads(done));
        drop(in_flight);
    }
}
