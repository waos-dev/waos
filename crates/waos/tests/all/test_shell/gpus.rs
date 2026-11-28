//! The system's GPUs as the test shell keeps them: one set of decoders, whatever GPU a program's
//! device is on, as on a system whose decoders' frames any GPU imports, and each GPU asked for
//! recorded in order.

use std::sync::{Arc, Mutex};

use waos::{GpuId, ShellGpus, ShellVideoDecoders};
use waos_headless::testing::Changes;

use super::video_decoders::TestVideoDecoders;

/// GPUs that hand out the same decoders, and keep each GPU they were asked for.
#[derive(Default)]
pub struct TestGpus {
    pub video_decoders: Arc<TestVideoDecoders>,
    asked: Mutex<Vec<Option<GpuId>>>,
    changes: Arc<Changes>,
}

impl TestGpus {
    /// GPUs asked for nothing, announcing each ask, and their decoders' calls, to `changes`.
    pub fn announcing(changes: &Arc<Changes>) -> TestGpus {
        TestGpus {
            video_decoders: Arc::new(TestVideoDecoders::announcing(changes)),
            asked: Mutex::default(),
            changes: Arc::clone(changes),
        }
    }

    /// The GPUs whose decoders were asked for, in order.
    pub fn asked(&self) -> Vec<Option<GpuId>> {
        self.asked.lock().unwrap().clone()
    }
}

impl ShellGpus for TestGpus {
    fn video_decoders(&self, gpu: Option<GpuId>) -> Option<Arc<dyn ShellVideoDecoders>> {
        self.asked.lock().unwrap().push(gpu);
        self.changes.announce();
        Some(Arc::clone(&self.video_decoders) as Arc<dyn ShellVideoDecoders>)
    }
}
