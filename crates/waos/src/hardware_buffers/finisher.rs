//! Waits for a view's frames off the program's thread, and hands each to the shell once the GPU
//! has drawn it. The thread polls the device for the submission that carried what the program
//! wrote before each frame a canvas presents, after every one that drew it, so the frame reaches
//! the shell as soon as it is drawn while the program goes on.
//! Each wait counts as the program's GPU work in flight until it is over, and as one of its view's
//! frames in flight until its hand-over is over, which paces the view's frames
//! (`views::frames_in_flight`). The thread ends when its canvas goes.
//!
//! The invariant: a frame is handed over here, on this thread, after its wait, and never from a
//! callback of wgpu-core's, which fires on whichever thread polls or submits: the program's
//! thread submits under the lock a freeze takes, and the hand-over reaches the lifecycle.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use waos_gpu_host::Device;

use crate::gpu::{Finishing, GpuInstance, wait_for, wgpu_core};
use crate::os::FinishHandOver;
use crate::views::FrameInFlight;

/// The canvas's handle on its thread.
pub(super) struct FrameFinisher {
    waits: Sender<Wait>,
}

/// One frame's wait for the thread, counted as in flight until it is over. A handle on the
/// device keeps the device while it is waited on.
pub(super) struct Wait {
    pub(super) device: Device,
    /// The submission that carried what the program wrote before the present, after every one
    /// that drew the frame; none, where wgpu-core refused it, waits for everything on the device
    /// so far.
    pub(super) submission: Option<wgpu_core::SubmissionIndex>,
    /// Post-wait work that finishes transferring pixels out of the GPU (e.g. offscreen staging
    /// readback on Linux), run before `hand_over`.
    pub(super) finish: Option<FinishHandOver>,
    /// The frame's hand-over to the shell, run once the wait is over.
    pub(super) hand_over: Box<dyn FnOnce() + Send>,
    /// The program's GPU work in flight, which the frame counts in until its wait is over.
    pub(super) in_flight: Finishing,
    /// The view's frames in flight, which the frame counts in until it is handed over.
    pub(super) frame: FrameInFlight,
}

impl FrameFinisher {
    /// Starts the thread that waits on `gpu` for the view `view`'s work.
    pub(super) fn start(gpu: GpuInstance, view: u64) -> FrameFinisher {
        let (waits, waited) = mpsc::channel();
        thread::Builder::new()
            .name(format!("frames of view {view}"))
            .spawn(move || wait_for_each(&gpu, &waited))
            .expect("a thread to finish a view's frames on");
        FrameFinisher { waits }
    }

    /// Waits for `wait` on the thread, after everything handed to it before.
    pub(super) fn finish(&self, wait: Wait) {
        // The thread ends only when this sender is dropped.
        let _ = self.waits.send(wait);
    }
}

fn wait_for_each(gpu: &Arc<wgpu_core::global::Global>, waits: &Receiver<Wait>) {
    for wait in waits {
        wait_for(gpu, wait.device.device_id(), wait.submission);
        if let Some(finish) = wait.finish {
            finish(gpu);
        }
        (wait.hand_over)();
        drop(wait.frame);
        drop(wait.in_flight);
    }
}
