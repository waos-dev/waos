//! WaOS's own settings at a desktop's width, in Fluent's controls: Back to the settings' first
//! page and the page's title, then a settings card for each setting this system offers, its name
//! over what it does and its control at the end: whether apps show inside WaOS, whether
//! WaOS is kept in the Dock or the taskbar, whether WaOS reaches the phone's files, with
//! the button that opens the system's page for it, and whether computers on the network may reach
//! WaOS's dev server, with, while they may, the button that shows a code to pair one with and the
//! computers paired, each with a button that removes it.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use waft::{App, BuildContext, Entity, Listener, StatelessWidget, WidgetRef};
use waft_winui::ThemeResources;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use super::developer_access_said;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use super::fluent_page::{ListRow, list_block};
use super::fluent_page::{Start, card_button, card_switch, page, setting_card};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::logic::developer_access::PairedComputer;
use crate::state::library::Library;
use crate::ui::library::words;

/// WaOS's own settings, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentGeneralSettings {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for FluentGeneralSettings {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let shown = Shown {
            library: self.library.clone(),
        };
        let cards = [
            shown.where_programs_open(app),
            shown.where_waos_stays(app),
            shown.phone_files(app),
        ]
        .into_iter()
        .flatten();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let cards = cards.chain(shown.developer_access(app, &resources));
        let cards: Vec<WidgetRef> = cards.collect();
        page(&self.library, &resources, &[words::WAOS_SETTINGS], cards)
    }
}

/// What one build of the page reads and writes.
struct Shown {
    library: Entity<Library>,
}

impl Shown {
    /// Where WaOS offers the choice, whether apps show inside WaOS, over what the choice does.
    fn where_programs_open(&self, app: &App) -> Option<WidgetRef> {
        let inside = self.library.read(app).programs_inside_waos()?;
        let library = self.library.clone();
        let toggle = card_switch(inside, move |app, inside| {
            library.update(app, |library, cx| {
                library.show_programs_inside_waos(inside, cx)
            });
        });
        Some(self.setting(
            words::PROGRAMS_INSIDE_WAOS,
            words::PROGRAMS_INSIDE_WAOS_DETAIL,
            toggle,
        ))
    }

    /// In a desktop's launcher, whether WaOS is kept in the Dock or the taskbar, over what
    /// turning it off does.
    fn where_waos_stays(&self, app: &App) -> Option<WidgetRef> {
        let kept = self.library.read(app).kept_in_dock()?;
        let platform = app.platform().target_platform();
        let library = self.library.clone();
        let toggle = card_switch(kept, move |app, kept| {
            library.update(app, |library, cx| library.keep_in_dock(kept, cx));
        });
        Some(self.setting(
            words::keep_in_dock(platform),
            words::keep_in_dock_detail(platform),
            toggle,
        ))
    }

    /// Where WaOS reaches the phone's files only as the user allows, whether it may, and a
    /// button that opens the system's page for it.
    fn phone_files(&self, app: &App) -> Option<WidgetRef> {
        let reachable = self.library.read(app).phone_files_reachable(app)?;
        let library = self.library.clone();
        let open = Listener::new(move |app| {
            library.update(app, |library, cx| library.open_phone_files_page(cx));
        });
        let said = if reachable {
            words::PHONE_FILES_ON
        } else {
            words::PHONE_FILES_OFF
        };
        let button = card_button(words::OPEN_ANDROID_SETTINGS, open);
        Some(self.setting(words::PHONE_FILES, said, button))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    /// Where WaOS offers it, whether computers on the network may reach WaOS's dev server, over
    /// what that does and whether it listens; and while they may, pairing a computer and the
    /// computers paired.
    fn developer_access(&self, app: &App, resources: &ThemeResources) -> Vec<WidgetRef> {
        let library = self.library.read(app);
        let Some(on) = library.developer_access_on() else {
            return Vec::new();
        };
        let said = developer_access_said(library.developer_listening());
        let entity = self.library.clone();
        let toggle = card_switch(on, move |app, on| {
            entity.update(app, |library, cx| library.turn_developer_access(on, cx));
        });
        let switch = self.setting(words::DEVELOPER_ACCESS, &said, toggle);
        if !on {
            return vec![switch];
        }
        let paired = library.paired_computers().unwrap_or_default();
        let pairing = self.pairing(library.pairing_code());
        vec![switch, pairing, self.paired_computers(paired, resources)]
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    /// The code shown for a computer to pair with, or the button that shows one.
    fn pairing(&self, code: Option<waos_dev_protocol::PairingCode>) -> WidgetRef {
        let library = self.library.clone();
        let show = Listener::new(move |app| {
            library.update(app, |library, cx| library.show_pairing_code(cx));
        });
        let said = match &code {
            Some(code) => words::pairing_code(code.as_str()),
            None => words::PAIR_A_COMPUTER_DETAIL.to_owned(),
        };
        let button = card_button(words::SHOW_A_CODE, show);
        self.setting(words::PAIR_A_COMPUTER, &said, button)
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    /// The computers `paired`, each with the button that removes it; or that there are none.
    fn paired_computers(
        &self,
        paired: Vec<PairedComputer>,
        resources: &ThemeResources,
    ) -> WidgetRef {
        let detail = paired.is_empty().then_some(words::NO_PAIRED_COMPUTERS);
        let rows = paired
            .into_iter()
            .map(|computer| {
                let library = self.library.clone();
                let name = computer.name.clone();
                let remove = Listener::new(move |app| {
                    library.update(app, |library, cx| library.remove_paired_computer(&name, cx));
                });
                ListRow {
                    text: computer.name,
                    control: Some(card_button(words::REMOVE, remove)),
                }
            })
            .collect();
        list_block(
            resources,
            Start::Edge,
            words::PAIRED_COMPUTERS,
            detail,
            None,
            rows,
        )
    }

    /// A setting's card: its `name` over what it does, and its `control`.
    fn setting(&self, name: &str, detail: &str, control: WidgetRef) -> WidgetRef {
        setting_card(Start::Edge, name, Some(detail), Some(control))
    }
}
