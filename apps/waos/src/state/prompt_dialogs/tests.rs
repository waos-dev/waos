use crate::state::test_turns::{Turn, turn_until};
use std::cell::{Cell, RefCell};
use std::path::Path;

use waft::ui::PlatformRef;
use waft::ui::{EmbedderClient, View as _};
use waft::{IntoWidget, Shell, SizedBox};
use waft_test::{TestPlatform, TestWindows};
use waos::{ProgramEnvironment, WaOS};
use waos_package::ProgramId;
use waos_window_messages::ToWindow;

use super::*;
use crate::logic::macos_shell::{
    Compositing, LaunchAttempt, Launches, Served, launch_failed, said_hello, window_process,
};
use crate::logic::test_engine::{self, NoWindows};

/// The id forever runs under, as an installed program's.
const FOREVER: &str = "dev.example.forever";

/// Forever, which has no window and runs until stopped, under its id.
fn forever(engine: &WaOS) -> RunningProgram {
    let forever = engine
        .load(
            Path::new(waos_test_programs::FOREVER),
            &waos_test_programs::code_of(waos_test_programs::FOREVER),
        )
        .expect("forever loads")
        .named(FOREVER);

    engine
        .run(&forever, ProgramEnvironment::default())
        .expect("forever runs")
}

fn forever_id() -> ProgramId {
    FOREVER.parse().unwrap()
}

/// WaOS's prompts on macOS, over the apps `reach` reaches, on a platform with a compositor and
/// windows of the test's own.
struct Prompting {
    shell: Shell,
    platform: Rc<TestPlatform>,
    dialogs: PromptDialogs,
    windows: Rc<TestWindows>,
    /// Where the test reports what a dialog does, as the programs' apps do.
    reports: mpsc::UnboundedSender<DialogReport>,
}

impl Prompting {
    fn new(reach: &Arc<AppReach>) -> Prompting {
        let windows = Rc::new(TestWindows::new());
        let platform = Rc::new(
            TestPlatform::new()
                .with_compositor(Arc::new(Compositing))
                .with_windows(Rc::clone(&windows)),
        );
        let made = Rc::new(RefCell::new(None));
        let (kept, reach) = (Rc::clone(&made), Arc::clone(reach));
        let (reports, reported) = mpsc::unbounded();
        let shell = Shell::new(Rc::clone(&platform) as PlatformRef, move |app| {
            let compositor = app.platform().compositor().expect("a compositor");
            *kept.borrow_mut() = Some(PromptDialogs::new(app, reach, compositor, reported));
        });
        let dialogs = made.borrow_mut().take().expect("the dialogs are made");
        Prompting {
            shell,
            platform,
            dialogs,
            windows,
            reports,
        }
    }

    /// Shows a prompt for `program`, which notes in `dismissed` if it is put away unanswered.
    fn show(&self, program: &RunningProgram, dismissed: &Rc<Cell<bool>>) -> ShownPrompt {
        let dismissed = Rc::clone(dismissed);
        let mut app = self.shell.app();
        self.dialogs.show(
            &mut app,
            program,
            SizedBox::shrink().into_widget(),
            Rc::new(move |_app| dismissed.set(true)),
        )
    }

    /// Runs the app's tasks as they wake until `done`, failing after `WITHIN`.
    fn run_tasks_until(&self, what: &str, mut done: impl FnMut() -> bool) {
        turn_until(&self.platform, what, || {
            self.shell.cell().checkpoint();
            if done() { Turn::Done } else { Turn::Idle }
        });
    }
}

#[test]
fn a_prompt_of_a_program_with_no_app_launches_the_app_and_shows_there_not_in_waos() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _within = runtime.enter();
    let launches = Launches::default();
    let reach = launches.reach();
    let engine = test_engine::engine(Arc::new(NoWindows));
    let program = forever(&engine);
    let prompting = Prompting::new(&reach);

    let dismissed = Rc::new(Cell::new(false));
    let _shown = prompting.show(&program, &dismissed);
    assert_eq!(
        launches.asked(),
        [(forever_id(), LaunchAttempt::First)],
        "the prompt launches the program's app"
    );
    let Served {
        process, mut sent, ..
    } = window_process(program.clone());
    said_hello(&reach, &forever_id(), &process);
    let mut asked = None;
    prompting.run_tasks_until("the prompt's dialog asked of the app", || {
        while let Ok(message) = sent.try_recv() {
            if let ToWindow::ShowPrompt { prompt, size, .. } = message {
                asked = Some((prompt, size));
            }
        }
        asked.is_some()
    });
    assert_eq!(
        asked,
        Some((1, PROMPT_SIZE)),
        "shown in a dialog of the app"
    );
    assert!(
        prompting.windows.made().is_empty(),
        "no window of WaOS's shows the prompt"
    );
    assert!(!dismissed.get());
    program.stop();
}

#[test]
fn a_prompts_view_arrives_as_every_view_and_is_built_from_the_data_naming_the_prompt() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _within = runtime.enter();
    let launches = Launches::default();
    let reach = launches.reach();
    let engine = test_engine::engine(Arc::new(NoWindows));
    let program = forever(&engine);
    let mut prompting = Prompting::new(&reach);
    let dismissed = Rc::new(Cell::new(false));
    let _shown = prompting.show(&program, &dismissed);
    let Served { process, .. } = window_process(program.clone());
    said_hello(&reach, &forever_id(), &process);
    let metrics = ViewMetrics {
        physical_size: [760.0, 400.0],
        device_pixel_ratio: 2.0,
        ..ViewMetrics::default()
    };
    let mut shown_report = Some(DialogReport {
        prompt: 1,
        report: PromptReport::Shown(metrics),
    });
    prompting.run_tasks_until("the prompt's view asked for", || {
        let dialog_shown = !prompting
            .dialogs
            .shown
            .read(&prompting.shell.app())
            .dialogs
            .is_empty();
        if dialog_shown && let Some(report) = shown_report.take() {
            prompting.reports.unbounded_send(report).unwrap();
        }
        !prompting.platform.added_views().is_empty()
    });

    let view = Rc::clone(&prompting.platform.added_views()[0]);
    assert_eq!(
        view.initial_data(),
        Some(dialog_data(1)),
        "the data names the prompt"
    );
    prompting.shell.view_added(view.id());
    let view: ViewRef = view;
    let content = prompting
        .dialogs
        .shown
        .read(&prompting.shell.app())
        .content_of(&view);
    assert!(
        content.is_some(),
        "the prompt is built in the view that arrived"
    );
    assert!(!dismissed.get());
    program.stop();
}

#[test]
fn a_prompt_whose_app_never_comes_is_put_away_unanswered() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _within = runtime.enter();
    let launches = Launches::default();
    let reach = launches.reach();
    let engine = test_engine::engine(Arc::new(NoWindows));
    let program = forever(&engine);
    let prompting = Prompting::new(&reach);

    let dismissed = Rc::new(Cell::new(false));
    let _shown = prompting.show(&program, &dismissed);
    launch_failed(&reach, &forever_id(), "Launch Services: no app");
    launch_failed(&reach, &forever_id(), "Launch Services: no app");
    prompting.run_tasks_until("the prompt put away", || dismissed.get());
    assert!(
        prompting.windows.made().is_empty(),
        "nor is it shown in a window of WaOS's"
    );
    program.stop();
}
