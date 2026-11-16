//! How media comes to be opened: a video picked in the system's file dialog, a link typed in the
//! Open Location dialog, or a file or link the user opened with the app; and how a folder of
//! videos comes into the library, picked in the system's folder dialog.

use std::rc::Rc;

use waft_embedder::{FileType, FolderOptions, OpenOptions, OpenRequest, PickedFolder};
use waft_foundation::{App, Entity, Ref};
use waft_widgets::{BuildContext, Text, TextEditingController, TextEditingControllerLeaf};
use waft_winui::{ContentDialog, ContentDialogButton, ContentDialogResult, TextBox};

use crate::library::{self, Library};
use crate::playback::{self, Playback};
use crate::source::Source;
use crate::video_types::{EXTENSIONS, MIME_TYPES};

/// Apple's example of an HLS stream, in fragmented MP4 at several bitrates, with subtitles and
/// audio in several languages: what the Open Location dialog offers.
const EXAMPLE_STREAM: &str = "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8";

/// The videos the file dialog offers: what Apple's players play.
fn video_types() -> Vec<FileType> {
    let text = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
    vec![FileType {
        label: "Movies".to_owned(),
        extensions: text(&EXTENSIONS),
        mime_types: text(&MIME_TYPES),
    }]
}

/// Asks for a video in the system's file dialog, a sheet on the window, and plays the one picked.
pub(crate) fn open_file(playback: &Entity<Playback>, app: &mut App) {
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        return eprintln!("this system has no file dialogs");
    };
    let window = platform.views().first().map(|view| view.id());
    let options = OpenOptions {
        types: video_types(),
        ..OpenOptions::default()
    };
    let picked = dialogs.open_file(window, options);
    let playback = playback.clone();
    app.spawn(async move |cx| match picked.await {
        Ok(Some(file)) => match Source::picked(&file) {
            Some(source) => cx.update(|app| playback::open(&playback, app, source)),
            None => eprintln!("the system gave the video picked no path"),
        },
        Ok(None) => {}
        Err(error) => eprintln!("{error}"),
    });
}

/// Asks for a folder of videos in the system's folder dialog, a sheet on the window, and adds the
/// one picked to the library.
pub(crate) fn add_folder(library: &Entity<Library>, app: &mut App) {
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        return eprintln!("this system has no file dialogs");
    };
    let window = platform.views().first().map(|view| view.id());
    let options = FolderOptions {
        confirm_label: Some("Add".to_owned()),
        ..FolderOptions::default()
    };
    let picked = dialogs.open_folder(window, options);
    let library = library.clone();
    app.spawn(async move |cx| match picked.await {
        Ok(Some(PickedFolder::Path(folder))) => {
            cx.update(|app| library::add_folder(&library, app, &folder));
        }
        Ok(Some(PickedFolder::Handle(_))) => eprintln!("the system gave the folder picked no path"),
        Ok(None) => {}
        Err(error) => eprintln!("{error}"),
    });
}

/// Asks for a link in Fluent's dialog over the page `context` is in, its field holding Apple's
/// example stream, and plays the link given.
pub(crate) fn open_location(playback: &Entity<Playback>, app: &mut App, context: BuildContext) {
    let link = TextEditingController::new(app).text(app, EXAMPLE_STREAM);
    let field = TextBox::new(link.erase())
        .placeholder_text("https://")
        .autofocus(true);
    let dialog = ContentDialog::new(app);
    dialog.title(app, Text::new("Open Location"));
    dialog.content(app, field);
    dialog.primary_button_text(app, "Open");
    dialog.close_button_text(app, "Cancel");
    dialog.default_button(app, ContentDialogButton::Primary);
    let shown = match dialog.show_async(app, context) {
        Ok(shown) => shown,
        Err(error) => {
            eprintln!("the Open Location dialog is not shown: {error:?}");
            return let_go(app, dialog, link);
        }
    };
    let playback = playback.clone();
    app.spawn(async move |cx| {
        let answered = shown.await;
        cx.update(|app| {
            let typed = link.text_value(app).to_owned();
            let_go(app, dialog, link);
            if answered != ContentDialogResult::Primary {
                return;
            }
            match Source::link(&typed) {
                Some(source) => playback::open(&playback, app, source),
                None => eprintln!("not a link to open: {typed}"),
            }
        });
    });
}

/// Lets go of the Open Location dialog and of its field's text.
fn let_go(app: &mut App, dialog: Ref<ContentDialog>, link: Ref<TextEditingController>) {
    dialog.dispose(app);
    link.dispose(app);
}

/// Plays what the user opens with the app from now on, such as a video opened with it in the
/// Finder: the first file, or else the first link.
pub(crate) fn play_what_is_opened_with_the_app(playback: &Entity<Playback>, app: &mut App) {
    let playback = playback.clone();
    app.set_open_requested_handler(Some(Rc::new(move |app: &mut App, request: OpenRequest| {
        if let Some(source) = source_of(&request) {
            playback::open(&playback, app, source);
        }
    })));
}

/// What `request` opens: its first file, or else its first link.
fn source_of(request: &OpenRequest) -> Option<Source> {
    let file = request.files.iter().find_map(Source::picked);
    file.or_else(|| request.links.iter().find_map(|link| Source::link(link)))
}
