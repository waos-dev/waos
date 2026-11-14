//! Where this backend's callbacks run.
//!
//! wgpu's callback-style calls (`map_async`, `on_submitted_work_done`, the device-lost and
//! uncaptured-error handlers) are `async` functions or streams in the WIT. Each becomes a task
//! spawned onto the program's executor, which runs whenever the program next awaits; nothing
//! here can run inside a synchronous `Device::poll`.

use std::future::Future;

pub(crate) fn spawn(task: impl Future<Output = ()> + 'static) {
    wasdk::spawn(task);
}
