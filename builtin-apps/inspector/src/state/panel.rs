//! A live panel's state: its rows, in the source its table shows, the row selected for the detail
//! pane, which of them its filters keep, whether and since when it listens, and the streams it
//! listens to. Each event heard is one change of the source, an insert or an update of one row,
//! so the table builds only the rows on screen and nothing is sent again.
//!
//! The invariant: the panel owns the end of its streams: stopping it drops what ends them, so
//! every stream of a listening is let go at once, and none outlives the panel's listening; and
//! the rows change only through the panel's log, an event at a time.

use std::future::Future;
use std::pin::pin;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{Either, Shared, select};
use waft::{App, Context, Entity, Handle, Ref};
use waft_winui::{TableViewSelection, TableViewSource};
use wasdk::StreamReader;

use crate::clock;
use crate::logic::panels::{Filter, PanelRow, Rows};

/// A panel's log: what turns what it hears into changes of its rows.
pub(crate) trait Log: Default + 'static {
    type Row: PanelRow;
}

/// Whether a panel listens, as far as it knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// It never listened: its program did not run.
    Nothing,
    /// It listens, or listened, from this time, in seconds since the epoch.
    Since(i64),
    /// WaOS did not start the program as asked, for this reason.
    Refused(String),
}

/// A listening, as each of its streams holds it: what ends the stream once the panel stops.
#[derive(Clone)]
pub(crate) struct Listening {
    stopped: Shared<oneshot::Receiver<()>>,
}

pub(crate) struct LivePanel<L: Log> {
    pub(crate) rows: Ref<TableViewSource<L::Row>>,
    /// The row the detail pane shows.
    pub(crate) selection: Ref<TableViewSelection>,
    log: L,
    pub(crate) filter: Filter<<L::Row as PanelRow>::Kind>,
    /// What the filter's field holds, lowercase.
    pub(crate) text: String,
    pub(crate) heard: Heard,
    /// Dropped to end every stream of the listening.
    listening: Option<oneshot::Sender<()>>,
}

impl<L: Log> LivePanel<L> {
    /// A panel that has not listened yet.
    pub(crate) fn new(app: &mut App) -> Entity<LivePanel<L>> {
        let rows = TableViewSource::from(app, Vec::new());
        let selection = TableViewSelection::new(app);
        app.new_entity(|_cx| LivePanel {
            rows,
            selection,
            log: L::default(),
            filter: Filter::All,
            text: String::new(),
            heard: Heard::Nothing,
            listening: None,
        })
    }

    /// Begins a listening, its rows and its log new; answers it, for its streams to hold.
    pub(crate) fn begin(&mut self, cx: &mut Context<LivePanel<L>>) -> Listening {
        self.stop();
        self.rows.handle().set_items(cx, Vec::new());
        self.log = L::default();
        self.heard = Heard::Since(clock::now());
        let (listening, stopped) = oneshot::channel();
        self.listening = Some(listening);
        cx.notify();
        Listening {
            stopped: stopped.shared(),
        }
    }

    /// Lets go of the rows heard so far; the listening goes on.
    pub(crate) fn clear(&mut self, cx: &mut Context<LivePanel<L>>) {
        self.rows.handle().set_items(cx, Vec::new());
        cx.notify();
    }

    /// Ends the listening's streams at once; the rows stay.
    pub(crate) fn stop(&mut self) {
        self.listening = None;
    }

    /// WaOS did not start the program as asked, for `why`.
    pub(crate) fn refused(&mut self, why: String, cx: &mut Context<LivePanel<L>>) {
        self.heard = Heard::Refused(why);
        cx.notify();
    }

    /// Tells the log something heard, which changes the rows through `tell`.
    pub(crate) fn tell(
        &mut self,
        cx: &mut Context<LivePanel<L>>,
        tell: impl FnOnce(&mut L, &mut SourceRows<'_, L::Row>),
    ) {
        let mut rows = SourceRows {
            source: self.rows.handle(),
            app: cx,
        };
        tell(&mut self.log, &mut rows);
        // The counts and the status read the rows; the table hears the source itself.
        cx.notify();
    }

    /// The rows, in the order heard.
    pub(crate) fn all<'a>(&self, app: &'a App) -> impl Iterator<Item = &'a L::Row> {
        let source = self.rows.handle();
        (0..source.item_count(app)).map(move |index| source.item(app, index))
    }

    /// The row the detail pane shows.
    pub(crate) fn selected<'a>(&self, app: &'a App) -> Option<&'a L::Row> {
        let index = self.selection.handle().selected_item(app)?;
        Some(self.rows.handle().item(app, index))
    }
}

/// Stops `panel`, which goes out of sight: its streams end at once, and no row stays selected, so
/// the table that shows it again binds a selection with nothing to clear.
pub(crate) fn stop<L: Log>(app: &mut App, panel: &Entity<LivePanel<L>>) {
    let selection = panel.read(app).selection.handle();
    selection.deselect_all(app);
    panel.update(app, |panel, _cx| panel.stop());
}

/// Shows the rows `filter` keeps, with the filter's field's text.
pub(crate) fn set_filter<L: Log>(
    app: &mut App,
    panel: &Entity<LivePanel<L>>,
    filter: Filter<<L::Row as PanelRow>::Kind>,
    text: &str,
) {
    let text = text.to_lowercase();
    let rows = panel.update(app, |panel, cx| {
        panel.filter = filter;
        panel.text = text.clone();
        cx.notify();
        panel.rows.handle()
    });
    if filter == Filter::All && text.is_empty() {
        rows.clear_filter(app);
    } else {
        rows.filter(app, move |row| filter.keeps(row, &text));
    }
}

/// Hears `events` into `panel` until they end or the panel stops, each told to its log by `tell`.
pub(crate) fn hear<L: Log, E: 'static>(
    app: &mut App,
    panel: &Entity<LivePanel<L>>,
    listening: Listening,
    mut events: StreamReader<E>,
    tell: fn(&mut L, &mut SourceRows<'_, L::Row>, E),
) {
    let panel = panel.clone();
    app.spawn(async move |cx| {
        let Listening { mut stopped } = listening;
        loop {
            let Some(event) = until(events.next(), &mut stopped).await.flatten() else {
                return;
            };
            cx.update(|app| {
                panel.update(app, |panel, cx| {
                    panel.tell(cx, |log, rows| tell(log, rows, event));
                });
            });
        }
    });
}

/// What `next` answers, or none once `stopped` resolves first.
async fn until<T>(
    next: impl Future<Output = T>,
    stopped: &mut Shared<oneshot::Receiver<()>>,
) -> Option<T> {
    match select(pin!(next), stopped).await {
        Either::Left((answer, _)) => Some(answer),
        Either::Right(_) => None,
    }
}

/// A panel's rows in its table's source: each change of them, one change of the source.
pub(crate) struct SourceRows<'a, R: 'static> {
    source: Handle<TableViewSource<R>>,
    app: &'a mut App,
}

impl<R: 'static> Rows<R> for SourceRows<'_, R> {
    fn len(&self) -> usize {
        self.source.item_count(self.app)
    }

    fn get(&self, index: usize) -> &R {
        self.source.item(self.app, index)
    }

    fn insert(&mut self, index: usize, row: R) {
        self.source.insert(self.app, index, row);
    }

    fn update(&mut self, index: usize, change: impl FnOnce(&mut R)) {
        self.source.update(self.app, index, change);
    }

    fn remove(&mut self, index: usize) {
        self.source.remove(self.app, index);
    }

    fn clear(&mut self) {
        self.source.set_items(self.app, Vec::new());
    }
}
