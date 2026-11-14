//! The app's own folders on Android: its cache folder, `Context.getCacheDir`, which the system
//! clears when it needs room, for the engine's code cache; and its files folder,
//! `Context.getFilesDir`, which nothing clears, for the store and WaOS's preferences. Both are the
//! app's alone.

// The VM and the application reach the app as raw pointers, which JNI takes back as its own
// handles; that is the `unsafe` here.
#![allow(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use jni::objects::{JObject, JString};
use jni::{Env, jni_sig, jni_str};
use waft::AndroidApp;

use crate::logic::folders::AppFolders;

/// The caches under the app's cache folder, and the store and the preferences under its files
/// folder.
pub(crate) fn app_folders(app: &AndroidApp) -> Result<AppFolders> {
    let files = app
        .internal_data_path()
        .context("Android named no files folder for WaOS")?;
    let caches = cache_folder(app).context("reading WaOS's cache folder from Android")?;
    Ok(AppFolders {
        code_cache: caches.join("code"),
        package_copies: caches.join("packages"),
        store: files.join("store"),
        preferences: files.join("preferences.toml"),
    })
}

/// `Context.getCacheDir()`, as the application answers it.
fn cache_folder(app: &AndroidApp) -> jni::errors::Result<PathBuf> {
    // SAFETY: this process's own VM, as Waft's Android host hands it out.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| {
        // SAFETY: an unowned global reference to the application, borrowed for this frame.
        let context = unsafe { JObject::from_raw(env, app.application_as_ptr().cast()) };
        let folder = env
            .call_method(
                &context,
                jni_str!("getCacheDir"),
                jni_sig!("()Ljava/io/File;"),
                &[],
            )?
            .l()?;
        let path = env
            .call_method(
                &folder,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        Ok(PathBuf::from(string(env, path)?))
    })
}

fn string(env: &mut Env<'_>, object: JObject<'_>) -> jni::errors::Result<String> {
    env.cast_local::<JString>(object)?.try_to_string(env)
}
