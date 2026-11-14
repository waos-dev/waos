//! Where a call that reaches the system's audio through cpal runs.
//!
//! On Android cpal asks the system's audio manager through Java, to list the devices and to build
//! a stream, and Android's runtime lets a thread into Java on the thread's own stack alone: it
//! reads the stack's bounds from the thread as it attaches it, and aborts the process when the
//! call is not within them (`art::Thread::InitStack`); and it refuses each call into Java made
//! beneath them as a stack overflow (`art::ArtMethod::Invoke`). The engine calls a provider from
//! a program's thread on a stack of wasmtime's own, a fiber's, which a program's code and the
//! host calls it makes run on. So on Android each such call runs on a thread of its own, which
//! cpal attaches to Java for the call and which detaches as it ends; everywhere else it runs
//! where it is called.
//!
//! The invariant: every use of cpal that may reach Java goes through [`on_a_threads_own`].
//! Listing devices and building a stream do; playing, pausing and closing a stream are AAudio's
//! alone, with no Java.

/// Runs `call` on a stack that is a thread's own, and answers what it answers; its panic is the
/// caller's.
#[cfg(target_os = "android")]
pub(crate) fn on_a_threads_own<T: Send>(call: impl FnOnce() -> T + Send) -> T {
    on_a_thread_of_its_own(call)
}

/// Runs `call` where it is called: only Android's audio is reached through Java.
#[cfg(not(target_os = "android"))]
pub(crate) fn on_a_threads_own<T: Send>(call: impl FnOnce() -> T + Send) -> T {
    call()
}

/// Runs `call` on a thread started for it, waits for it, and answers what it answers; its panic
/// is the caller's.
#[cfg(any(target_os = "android", test))]
fn on_a_thread_of_its_own<T: Send>(call: impl FnOnce() -> T + Send) -> T {
    let answered = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("waos-audio-call".into())
            .spawn_scoped(scope, call)
            .expect("a thread to reach the system's audio on")
            .join()
    });
    answered.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::thread;

    use super::*;

    #[test]
    fn a_call_runs_on_another_thread_and_answers_here() {
        let borrowed = String::from("borrowed");
        let (thread, read) = on_a_thread_of_its_own(|| (thread::current().id(), borrowed.len()));
        assert_ne!(thread, thread::current().id());
        assert_eq!(read, 8);
    }

    #[test]
    fn a_calls_panic_is_the_callers() {
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            on_a_thread_of_its_own(|| panic!("the device failed"))
        }));
        let message = panicked.unwrap_err();
        assert_eq!(message.downcast_ref::<&str>(), Some(&"the device failed"));
    }
}
