//! The phone's files on Android: WaOS's "All files access" (`MANAGE_EXTERNAL_STORAGE`), which lets
//! it reach the phone's shared storage by its paths, read from `Environment.isExternalStorageManager`
//! each time it is asked; the page of Android's settings where the user turns it on for WaOS,
//! `Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION` at WaOS's package; and This computer's
//! home, the shared storage, `Environment.getExternalStorageDirectory`, while WaOS may reach it, and
//! WaOS's files folder before. Asked on the main thread alone (`state::phone_files`).

// The VM and the application reach the app as raw pointers, which JNI takes back as its own
// handles; that is the `unsafe` here.
#![allow(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use jni::objects::{JObject, JString, JValue};
use jni::refs::Global;
use jni::{Env, JavaVM, jni_sig, jni_str};
use waft::AndroidApp;

use crate::logic::phone_files::PhoneFilesAccess;

/// The action of Android's page for one app's All files access.
const SWITCH_PAGE: &str = "android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION";

/// `Intent.FLAG_ACTIVITY_NEW_TASK`: a page started from the application, not an activity.
const NEW_TASK: i32 = 0x1000_0000;

/// WaOS's All files access, asked of Android through the process's VM and application.
pub(crate) struct AllFilesAccess {
    vm: JavaVM,
    application: Global<JObject<'static>>,
    /// WaOS's files folder, This computer's home while the shared storage is out of reach.
    own_folder: PathBuf,
}

impl AllFilesAccess {
    /// WaOS's All files access, asked through `app`'s VM and application.
    pub(crate) fn of(app: &AndroidApp) -> Result<AllFilesAccess> {
        // SAFETY: this process's own VM, as Waft's Android host hands it out.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let application = vm
            .attach_current_thread(|env| {
                // SAFETY: an unowned global reference to the application, borrowed for this
                // frame, of which a reference of WaOS's own is kept.
                let context = unsafe { JObject::from_raw(env, app.application_as_ptr().cast()) };
                env.new_global_ref(&context)
            })
            .context("holding the application")?;
        let own_folder = app
            .internal_data_path()
            .context("Android named no files folder for WaOS")?;
        Ok(AllFilesAccess {
            vm,
            application,
            own_folder,
        })
    }

    /// `Environment.isExternalStorageManager()`.
    fn storage_manager(&self) -> jni::errors::Result<bool> {
        self.vm.attach_current_thread(|env| {
            env.call_static_method(
                jni_str!("android/os/Environment"),
                jni_str!("isExternalStorageManager"),
                jni_sig!("()Z"),
                &[],
            )?
            .z()
        })
    }

    /// `Environment.getExternalStorageDirectory()`, the shared storage of the phone's user.
    fn shared_storage(&self) -> jni::errors::Result<PathBuf> {
        self.vm.attach_current_thread(|env| {
            let folder = env
                .call_static_method(
                    jni_str!("android/os/Environment"),
                    jni_str!("getExternalStorageDirectory"),
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

    /// Starts Android's page for WaOS's All files access, in a task of its own, as the
    /// application starts it.
    fn start_switch_page(&self) -> jni::errors::Result<()> {
        self.vm.attach_current_thread(|env| {
            let package = env
                .call_method(
                    &self.application,
                    jni_str!("getPackageName"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let package = string(env, package)?;
            let address = env.new_string(format!("package:{package}"))?;
            let uri = env
                .call_static_method(
                    jni_str!("android/net/Uri"),
                    jni_str!("parse"),
                    jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
                    &[JValue::from(&address)],
                )?
                .l()?;
            let action = env.new_string(SWITCH_PAGE)?;
            let intent = env.new_object(
                jni_str!("android/content/Intent"),
                jni_sig!("(Ljava/lang/String;Landroid/net/Uri;)V"),
                &[JValue::from(&action), JValue::from(&uri)],
            )?;
            env.call_method(
                &intent,
                jni_str!("addFlags"),
                jni_sig!("(I)Landroid/content/Intent;"),
                &[JValue::Int(NEW_TASK)],
            )?;
            env.call_method(
                &self.application,
                jni_str!("startActivity"),
                jni_sig!("(Landroid/content/Intent;)V"),
                &[JValue::from(&intent)],
            )?;
            Ok(())
        })
    }
}

impl PhoneFilesAccess for AllFilesAccess {
    fn reachable(&self) -> bool {
        self.storage_manager().unwrap_or_else(|error| {
            tracing::warn!("Android did not say whether WaOS has All files access: {error}");
            false
        })
    }

    fn open_switch(&self) -> Result<(), String> {
        self.start_switch_page().map_err(|error| error.to_string())
    }

    fn home(&self) -> PathBuf {
        if !self.reachable() {
            return self.own_folder.clone();
        }
        self.shared_storage().unwrap_or_else(|error| {
            tracing::warn!("Android named no shared storage: {error}");
            self.own_folder.clone()
        })
    }
}

fn string(env: &mut Env<'_>, object: JObject<'_>) -> jni::errors::Result<String> {
    env.cast_local::<JString>(object)?.try_to_string(env)
}
