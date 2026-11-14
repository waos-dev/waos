//! Adding a feed: a dialog asks for its address, a feed's own or that of a site that names one;
//! Add fetches it while the dialog stays, turning a bar, and the dialog closes onto the new
//! feed's items once its document was read, or stays and says why it was not: no address, a feed
//! followed already, a server not reached, or no feed there.

use std::rc::Rc;

use url::Url;
use waft::{
    AnyTextEditingController, App, BuildContext, Column, CrossAxisAlignment, Entity, Handle,
    IntoWidget, MainAxisSize, SizedBox, StatelessWidget, Text, TextEditingController,
    TextEditingControllerLeaf, WidgetRef,
};
use waft_winui::{
    ContentDialog, ContentDialogButton, ContentDialogButtonClickEventArgs, ProgressBar,
    TextBlockStyle, TextBox, ThemeResources,
};

use crate::feed_address::{feed_address, host_name};
use crate::fetching::{FetchedDocument, NotFetched, find_feed};
use crate::library::Scope;
use crate::reader_app::ReaderApp;
use crate::refreshing::seconds_now;
use crate::showing::list_items;

const TITLE: &str = "Add Feed";
const ASKED: &str = "The address of a feed, or of a site that has one.";
const PLACEHOLDER: &str = "example.org/feed.xml";
const FOLLOWED_ALREADY: &str = "You follow this feed already.";

/// The room kept beneath the address for the bar or the refusal, in logical points, so that the
/// dialog does not change its size as they come and go.
const STATUS_ROOM: f64 = 20.0;

/// How far adding has come, as the dialog shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Adding {
    #[default]
    Asking,
    /// The address given is being fetched.
    Looking,
    /// Why the address given was not added.
    Refused(String),
}

/// What the dialog is made of: the address typed and how far adding has come.
#[derive(Clone)]
struct Form {
    address: AnyTextEditingController,
    adding: Entity<Adding>,
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Form").finish_non_exhaustive()
    }
}

/// Asks for a feed's address over the window, and adds the feed it names; nothing where the
/// window is not built yet, or another dialog is open.
pub(crate) fn ask_for_a_feed(app: &mut App, reader: &ReaderApp) {
    let Some(context) = reader.navigator.current_context(app) else {
        return;
    };
    let form = Form {
        address: TextEditingController::new(app).as_controller(),
        adding: app.new_entity(|_cx| Adding::default()),
    };
    let dialog = dialog_asking(app, reader, &form);
    let Ok(shown) = dialog.show_async(app, context) else {
        return dialog.dispose(app);
    };
    app.spawn(async move |cx| {
        shown.await;
        cx.update(|app| dialog.dispose(app));
    });
}

fn dialog_asking(app: &mut App, reader: &ReaderApp, form: &Form) -> Handle<ContentDialog> {
    let (reader, adding_form) = (reader.clone(), form.clone());
    let dialog = ContentDialog::new(app);
    dialog
        .title(app, Text::new(TITLE))
        .content(app, FormView { form: form.clone() })
        .primary_button_text(app, "Add")
        .close_button_text(app, "Cancel")
        .default_button(app, ContentDialogButton::Primary)
        .primary_button_click(app, move |app, args| {
            add_clicked(app, &reader, &adding_form, dialog, args);
        })
}

/// Add was clicked: an address that is none, or a feed followed already, keeps the dialog and
/// says so; any other is fetched while the dialog waits.
fn add_clicked(
    app: &mut App,
    reader: &ReaderApp,
    form: &Form,
    dialog: Handle<ContentDialog>,
    args: &Rc<ContentDialogButtonClickEventArgs>,
) {
    let typed = form.address.text_value(app).to_owned();
    let address = match feed_address(&typed) {
        Ok(address) if reader.library.read(app).feed_at(address.as_str()).is_none() => address,
        Ok(_) => return refuse(app, form, args, FOLLOWED_ALREADY.to_owned()),
        Err(bad) => return refuse(app, form, args, bad.said().to_owned()),
    };
    let Ok(deferral) = args.get_deferral() else {
        return;
    };
    set(app, form, Adding::Looking);
    dialog.is_primary_button_enabled(app, false);
    let (reader, form, args) = (reader.clone(), form.clone(), Rc::clone(args));
    app.spawn(async move |cx| {
        let found = find_feed(address).await;
        cx.update(|app| {
            match found.and_then(|(address, document)| follow(app, &reader, &address, document)) {
                Ok(()) => set(app, &form, Adding::Asking),
                Err(NotFetched(why)) => refuse(app, &form, &args, why),
            }
            dialog.is_primary_button_enabled(app, true);
            let _ = deferral.complete(app);
        });
    });
}

/// Follows the feed at `address`, whose `document` was just read, and lists its items.
fn follow(
    app: &mut App,
    reader: &ReaderApp,
    address: &Url,
    document: FetchedDocument,
) -> Result<(), NotFetched> {
    if reader.library.read(app).feed_at(address.as_str()).is_some() {
        return Err(NotFetched(FOLLOWED_ALREADY.to_owned()));
    }
    let now = seconds_now();
    let id = reader.change_library(app, |library| {
        let id = library.follow(address.to_string(), host_name(address));
        library.merge(id, document.feed, now);
        library.refreshed(id, document.validators, None, now);
        id
    });
    list_items(app, reader, Scope::Feed(id));
    Ok(())
}

/// Keeps the dialog open, saying `why` the address was not added.
fn refuse(app: &mut App, form: &Form, args: &ContentDialogButtonClickEventArgs, why: String) {
    args.set_cancel(true);
    set(app, form, Adding::Refused(why));
}

fn set(app: &mut App, form: &Form, adding: Adding) {
    form.adding.update(app, |shown, cx| {
        *shown = adding;
        cx.notify();
    });
}

/// The dialog's content: what is asked, the address, and beneath it a bar turning while the
/// address is fetched, or why it was not added.
#[derive(Debug)]
struct FormView {
    form: Form,
}

impl StatelessWidget for FormView {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let adding = self.form.adding.read(app).clone();
        let asked =
            TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary);
        let address = TextBox::new(self.form.address)
            .placeholder_text(PLACEHOLDER)
            .autofocus(true)
            .is_enabled(adding != Adding::Looking);
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(12.0)
            .children([
                Text::new(ASKED).style(asked).into_widget(),
                address.into_widget(),
                SizedBox::new()
                    .height(STATUS_ROOM)
                    .child(status(&resources, adding))
                    .into_widget(),
            ])
            .into_widget()
    }
}

/// What the dialog says beneath the address, in the room it always keeps for it.
fn status(resources: &ThemeResources, adding: Adding) -> WidgetRef {
    match adding {
        Adding::Asking => SizedBox::shrink().into_widget(),
        Adding::Looking => ProgressBar::new().is_indeterminate(true).into_widget(),
        Adding::Refused(why) => {
            let style =
                TextBlockStyle::Caption.text_style(resources, resources.system_fill_color_critical);
            Text::new(why).style(style).max_lines(2).into_widget()
        }
    }
}
