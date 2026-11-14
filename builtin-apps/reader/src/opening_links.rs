//! Opening a link outside the app, in the browser the system picks for it: an item's original,
//! and the links in its text. Only `http` and `https` links leave the app, which WaOS opens
//! without asking; the window says so where nothing opened one.

use waft::App;
use waft::ui::OpenError;
use waft_winui::InfoBarSeverity;

use crate::reader_app::ReaderApp;

const NO_OPENER: &str = "This system gives Reader no way to open a link.";
const NO_BROWSER: &str = "No app on this system opens web links.";

/// Opens `link` in the system's browser.
pub(crate) fn open_link(app: &mut App, reader: &ReaderApp, link: &str) {
    if !(link.starts_with("https://") || link.starts_with("http://")) {
        return;
    }
    let platform = app.platform();
    let Some(opener) = platform.opener() else {
        return reader.say(app, InfoBarSeverity::Warning, NO_OPENER);
    };
    let opening = opener.open_link(link);
    let reader = reader.clone();
    app.spawn(async move |cx| {
        let said = match opening.await {
            Ok(()) => return,
            Err(OpenError::NoApp) => NO_BROWSER.to_owned(),
            Err(OpenError::Failed(why)) => format!("The link did not open: {why}"),
        };
        cx.update(|app| reader.say(app, InfoBarSeverity::Warning, said));
    });
}

/// Opens the original of the item being read.
pub(crate) fn open_original(app: &mut App, reader: &ReaderApp) {
    let link = reader.browsing.read(app).reading().and_then(|item| {
        let listed = reader.library.read(app).item(item)?;
        listed.item.link.clone()
    });
    if let Some(link) = link {
        open_link(app, reader, &link);
    }
}
