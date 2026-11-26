//! The system's GPUs as the test shell keeps them: one set of decoders, whatever GPU a program's
//! device is on, as on a system whose decoders' frames any GPU imports, and each GPU asked for
//! recorded in order.

use std::sync::{Arc, Mutex};

use waos::{GpuId, ShellGpus, ShellVideoDecoders};

use super::video_decoders::TestVideoDecoders;

/// GPUs that hand out the same decoders, and keep each GPU they were asked for.
#[derive(Default)]
pub struct TestGpus {
    pub video_decoders: Arc<TestVideoDecoders>,
    asked: Mutex<Vec<Option<GpuId>>>,
}

impl TestGpus {
    /// The GPUs whose decoders were asked for, in order.
    pub fn asked(&self) -> Vec<Option<GpuId>> {
        self.asked.lock().unwrap().clone()
    }
}

impl ShellGpus for TestGpus {
    fn video_decoders(&self, gpu: Option<GpuId>) -> Option<Arc<dyn ShellVideoDecoders>> {
        self.asked.lock().unwrap().push(gpu);
        Some(Arc::clone(&self.video_decoders) as Arc<dyn ShellVideoDecoders>)
    }
}
