//! Android's shortcuts of WaOS's programs, through Waft's Android host (`AndroidPlatform::shortcuts`,
//! Android's `ShortcutManager`): each program's shortcut as a `ShortcutInfo` going by the program's
//! id, named after it, its icon an adaptive one, and a tap opening the program's first window in a
//! task of its own, the view its initial data names (`WindowRestoration`), or WaOS's own window
//! with the action that names the program (`logic::program_shortcuts`).
//!
//! The invariant a later editor will break: a shortcut opening a program's own task carries
//! exactly the initial data the program's first window has when WaOS asks for it, since Android
//! finds a task again by that data, so a tap on a running program's icon brings its task forward
//! rather than starting a second.

use std::rc::Rc;

use futures::FutureExt;
use futures::future::LocalBoxFuture;
use waft::ui::Platform;
use waft_embedder_android::{
    AndroidPlatform, DisabledReason, PublishedShortcut, ShortcutIcon, ShortcutInfo, ShortcutMatch,
    ShortcutTarget, Shortcuts,
};
use waos_package::ProgramId;
use waos_program_window::{WindowRestoration, WindowRole};

use crate::logic::program_shortcuts::{
    KeptShortcut, ProgramShortcut, ShortcutOpens, SystemShortcuts, action_of, program_of_action,
};

/// What a tap on a removed program's greyed-out icon says.
const REMOVED: &str = "This program was removed from WaOS";

/// Android's shortcuts, where the platform is Waft's Android host's.
pub(crate) fn system_shortcuts(platform: &dyn Platform) -> Option<Rc<dyn SystemShortcuts>> {
    let android = platform.downcast_ref::<AndroidPlatform>()?;
    Some(Rc::new(AndroidShortcuts {
        shortcuts: android.shortcuts().clone(),
    }))
}

/// The app's shortcuts, as Android keeps them.
struct AndroidShortcuts {
    shortcuts: Shortcuts,
}

impl SystemShortcuts for AndroidShortcuts {
    fn pins(&self) -> bool {
        self.shortcuts.is_request_pin_supported()
    }

    fn icon_size(&self) -> u32 {
        self.shortcuts.icon_max_size()
    }

    fn request_pin(
        &self,
        shortcut: &ProgramShortcut,
    ) -> LocalBoxFuture<'static, Result<(), String>> {
        let requested = self.shortcuts.request_pin(&info_of(shortcut));
        async move { requested.await.map_err(|error| error.to_string()) }.boxed_local()
    }

    fn add_recent(&self, shortcut: &ProgramShortcut) {
        self.shortcuts.push_dynamic(&info_of(shortcut));
        self.shortcuts.report_used(shortcut.id.as_str());
    }

    fn kept(&self) -> LocalBoxFuture<'static, Result<Vec<KeptShortcut>, String>> {
        let every = ShortcutMatch {
            pinned: true,
            dynamic: true,
        };
        let published = self.shortcuts.shortcuts(every);
        async move {
            let published = published.await.map_err(|error| error.to_string())?;
            Ok(published.iter().map(kept_of).collect())
        }
        .boxed_local()
    }

    fn update(&self, shortcuts: &[ProgramShortcut]) -> LocalBoxFuture<'static, Result<(), String>> {
        let infos = shortcuts.iter().map(info_of).collect::<Vec<_>>();
        let updated = self.shortcuts.update(&infos);
        async move { updated.await.map_err(|error| error.to_string()) }.boxed_local()
    }

    fn enable(&self, ids: &[ProgramId]) {
        let ids = ids.iter().map(ProgramId::as_str).collect::<Vec<_>>();
        self.shortcuts.enable(&ids);
    }

    fn disable(&self, ids: &[String]) {
        let ids = ids.iter().map(String::as_str).collect::<Vec<_>>();
        self.shortcuts.remove_dynamic(&ids);
        self.shortcuts.disable(&ids, Some(REMOVED));
    }

    fn hear_taps(&self, tapped: Box<dyn Fn(ProgramId)>) {
        self.shortcuts.set_activation_handler(Some(Rc::new(
            move |activation| match program_of_action(&activation.action) {
                Some(id) => tapped(id),
                None => tracing::info!(
                    action = activation.action,
                    "a shortcut's tap names no program"
                ),
            },
        )));
    }
}

/// `shortcut` as Android's.
fn info_of(shortcut: &ProgramShortcut) -> ShortcutInfo {
    ShortcutInfo {
        id: shortcut.id.to_string(),
        short_label: shortcut.name.clone(),
        long_label: None,
        disabled_message: Some(REMOVED.to_owned()),
        icon: shortcut.icon.clone().map(ShortcutIcon::Adaptive),
        target: target_of(&shortcut.id, shortcut.opens),
        rank: Some(0),
    }
}

/// What a tap on the program `id`'s shortcut opens, as `opens` says.
fn target_of(id: &ProgramId, opens: ShortcutOpens) -> ShortcutTarget {
    match opens {
        ShortcutOpens::OwnTask => ShortcutTarget::View {
            initial_data: WindowRestoration {
                program: id.to_string(),
                role: WindowRole::First,
            }
            .initial_data(),
        },
        ShortcutOpens::InsideWaos => ShortcutTarget::App {
            action: action_of(id),
        },
    }
}

fn kept_of(published: &PublishedShortcut) -> KeptShortcut {
    KeptShortcut {
        id: published.id.clone(),
        pinned: published.pinned,
        enabled: published.enabled,
        disabled_by_waos: published.disabled_reason == Some(DisabledReason::ByApp),
    }
}
