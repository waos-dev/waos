//! The system's certificate checker on Android, which `rustls-platform-verifier` asks through its
//! Kotlin class, `org.rustls.platformverifier.CertificateVerifier`, in the package's own dex (the
//! AAR under `apps/waos/android`). The verifier finds the class through the application's class
//! loader, so it is handed the VM and the application once, before any connection is made: until
//! then its first check panics, and every HTTPS request, a source's or a program's, makes one.

// The VM and the application reach the app as raw pointers, which JNI takes back as its own
// handles; that is the `unsafe` here.
#![allow(unsafe_code)]

use jni::objects::JObject;
use waft::AndroidApp;

/// Hands `rustls-platform-verifier` the VM and the application, whose class loader finds its
/// Kotlin class in the package.
pub(crate) fn trust_the_system(app: &AndroidApp) -> jni::errors::Result<()> {
    // SAFETY: this process's own VM, as Waft's Android host hands it out.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| {
        // SAFETY: an unowned global reference to the application, borrowed for this frame;
        // `init_with_env` keeps a global reference of its own.
        let context = unsafe { JObject::from_raw(env, app.application_as_ptr().cast()) };
        rustls_platform_verifier::android::init_with_env(env, context)
    })
}
