//! What the window says of a command: that it was done, or why it could not be, in a Fluent info
//! bar above the document, until it is closed or the next command says something else.

use waft::{
    App, BuildContext, EdgeInsetsGeometry, IntoWidget, Padding, SizedBox, StatelessWidget,
    WidgetRef,
};
use waft_winui::{InfoBar, InfoBarSeverity};

use crate::documents_app::DocumentsApp;

/// What the window says above the document, until it is closed or the next command.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Notice {
    pub(crate) severity: InfoBarSeverity,
    pub(crate) message: String,
}

/// Says `message` above the document, as `severity` says.
pub(crate) fn say(
    app: &mut App,
    documents: &DocumentsApp,
    severity: InfoBarSeverity,
    message: impl Into<String>,
) {
    let notice = Notice {
        severity,
        message: message.into(),
    };
    documents.notice.update(app, |said, cx| {
        *said = Some(notice);
        cx.notify();
    });
}

/// Says nothing any more.
pub(crate) fn say_nothing(app: &mut App, documents: &DocumentsApp) {
    documents.notice.update(app, |said, cx| {
        if said.take().is_some() {
            cx.notify();
        }
    });
}

/// The bar that says the notice, where there is one.
#[derive(Debug)]
pub(crate) struct NoticeBar {
    pub(crate) documents: DocumentsApp,
}

impl StatelessWidget for NoticeBar {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let Some(notice) = self.documents.notice.read(app).clone() else {
            return SizedBox::new().into_widget();
        };
        let documents = self.documents.clone();
        let bar = InfoBar::new(true, move |app, open| {
            if !open {
                say_nothing(app, &documents);
            }
        })
        .severity(notice.severity)
        .message(notice.message);
        Padding::new(EdgeInsetsGeometry::from_ltrb(8.0, 0.0, 8.0, 8.0))
            .child(bar)
            .into_widget()
    }
}
