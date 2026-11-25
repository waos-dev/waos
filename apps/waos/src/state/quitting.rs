//! Quitting while programs run, by the rule of `logic::quit_question`: Waft's host asks the app
//! whether it may quit, and the app asks every program running through `request_exit`, each
//! unfrozen by the engine while it owes the answer. Once every one has answered, the app answers
//! the host: exit if every one agreed, having stopped every run, stopped listening, so the
//! socket file is gone, and kept every program's day; cancel if any refused, and nothing closes.
//! Every quit asks again, even while the programs decide an earlier one, and each is decided on
//! its own. A program that never answers keeps WaOS open until it is stopped, which the run's
//! `waos` does when its user presses Ctrl-C; a program that ends while asked has answered
//! exit.
//!
//! The invariant: the server is stopped and the days kept before exit is answered, never after,
//! by whichever quit was agreed first, and every quit agreed waits for it. On macOS the answer
//! ends the process without unwinding, so what is still there then stays: a socket file, and
//! programs stopped mid-frame; and the engine is never dropped, so its days are kept here.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::Shared;
use waft::ui::AppExitResponse;
use waft::{App, AppLifecycleListener, AsyncApp, Task};
use waos::{RunningProgram, WaOS};

use crate::logic::dev_server::DevServer;
use crate::logic::quit_question::every_program_agrees;
use crate::logic::runs::Runs;

/// From now on, a quit asks every program in `runs` first, and once they agree stops `server`
/// and keeps the days `engine` counted.
pub(crate) fn ask_programs_before_quitting(
    app: &mut App,
    runs: Arc<Runs>,
    server: DevServer,
    engine: Arc<WaOS>,
) {
    let stop = Rc::new(ServerStop::new(server, engine));
    AppLifecycleListener::new(app).on_exit_requested(
        app,
        Rc::new(move |app: &mut App| answer_quit(app, &runs, &stop)),
    );
}

/// Asks every program whether it may end, and answers once every one has: exit, once the
/// server is stopped, if every one agreed; cancel if any refused.
fn answer_quit(app: &mut App, runs: &Arc<Runs>, stop: &Rc<ServerStop>) -> Task<AppExitResponse> {
    tracing::info!("WaOS is asked to quit: asking every program");
    let (runs, stop) = (Arc::clone(runs), Rc::clone(stop));
    app.spawn(async move |cx| {
        let programs = runs.programs().await;
        let asked = programs.iter().map(RunningProgram::request_exit);
        if !every_program_agrees(asked).await {
            tracing::info!("a program refused: WaOS stays");
            return AppExitResponse::Cancel;
        }
        tracing::info!(
            programs = programs.len(),
            "every program agreed: WaOS stops what runs and quits"
        );
        stop.stop(cx).await;
        AppExitResponse::Exit
    })
}

/// The dev server, until the first quit every program agreed to stops it, and the engine whose
/// days it keeps then.
struct ServerStop {
    server: RefCell<Option<DevServer>>,
    engine: Arc<WaOS>,
    /// Told once the server is stopped.
    done: RefCell<Option<oneshot::Sender<()>>>,
    stopped: Shared<oneshot::Receiver<()>>,
}

impl ServerStop {
    fn new(server: DevServer, engine: Arc<WaOS>) -> ServerStop {
        let (done, stopped) = oneshot::channel();
        ServerStop {
            server: RefCell::new(Some(server)),
            engine,
            done: RefCell::new(Some(done)),
            stopped: stopped.shared(),
        }
    }

    /// Stops the server off the main thread, which stops every run and removes the socket file,
    /// then keeps every program's day, each run's end among them, unless a quit is doing so
    /// already; resolves once it is done.
    async fn stop(&self, cx: &AsyncApp) {
        let server = self.server.borrow_mut().take();
        if let Some(server) = server {
            let engine = Arc::clone(&self.engine);
            cx.run_in_background(move || {
                drop(server);
                engine.keep_every_day();
            })
            .await;
            let done = self.done.borrow_mut().take();
            if let Some(done) = done {
                // The other end is `stopped`'s, here as long as this is.
                let _ = done.send(());
            }
        }
        // Sent by whichever quit stopped the server, once it had.
        let _ = self.stopped.clone().await;
    }
}
