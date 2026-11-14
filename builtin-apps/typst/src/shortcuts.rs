//! The keys of the app's commands, heard by the window's app itself: a system with no menu bar
//! has nothing else to hear them. They are the app's shortcuts, above every widget, so a key
//! does its command whatever has the focus, and the text still takes the keys it edits with.
//! Each command's key is the one its menu item shows.

use std::rc::Rc;

use waft::{
    App, IntentRef, Listener, ShortcutActivatorRef, ShortcutMap, VoidCallbackIntent, WidgetsApp,
};

use crate::commands::{file_groups, view_groups};
use crate::documents_app::DocumentsApp;

/// The commands' keys, each doing its command on `documents`, then the keys every app has.
pub(crate) fn app_shortcuts(app: &App, documents: &DocumentsApp) -> ShortcutMap {
    let commands = file_groups().into_iter().chain(view_groups()).flatten();
    let keyed = commands.filter_map(|command| {
        let activator: ShortcutActivatorRef = Rc::new(command.chord?.activator(app));
        let (documents, run) = (documents.clone(), command.run);
        let doing = Listener::new(move |app: &mut App| run(app, &documents));
        let intent: IntentRef = Rc::new(VoidCallbackIntent::new(doing));
        Some((activator, intent))
    });
    keyed.chain(WidgetsApp::default_shortcuts(app)).collect()
}
