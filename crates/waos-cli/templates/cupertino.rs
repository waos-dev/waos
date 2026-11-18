// Waft's `StatefulWidget::State` takes `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use std::time::Duration;

use waft::*;
use waft_cupertino::*;

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        run_app(
            app,
            CupertinoApp::new()
                // Hides Flutter's DEBUG ribbon: `debug_show_checked_mode_banner(false)`.
                .debug_show_checked_mode_banner(false)
                .home(Home)
                .into_widget(),
        )
    })
    .await;
}

/// A typed line, the lines added below it, and a clock.
#[derive(Debug)]
struct Home;

/// What survives a rebuild: the field's controller, the lines added, and the time.
struct HomeState {
    state: StateData<Home>,
    entry: Option<Ref<TextEditingController>>,
    added: Vec<String>,
    now: DateTime,
}

impl StatefulWidget for Home {
    type State = HomeState;

    fn create_state(&self) -> HomeState {
        HomeState {
            state: StateData::new(),
            entry: None,
            added: Vec::new(),
            now: DateTime::now(),
        }
    }
}

impl State for HomeState {
    type Widget = Home;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).entry = Some(TextEditingController::new(app));
        // A task `spawn` starts waits on a timer each second, then reads the clock again.
        app.spawn(async move |cx| {
            loop {
                let wait = Completer::new();
                let future = wait.future();
                let second = Listener::new(move |app| wait.complete(app, ()));
                cx.update(|app| Timer::new(app, Duration::from_secs(1), second));
                future.await;
                cx.update(|app| self.set_state(app, |state| state.now = DateTime::now()));
            }
        });
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(entry) = app.get_mut(self).entry.take() {
            entry.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let entry = app.get(self).entry.clone().expect("made in init_state");
        let typed = entry.clone();
        // Add appends the field's line to the list, then empties the field for the next one.
        let add = Listener::new(move |app| {
            let line = typed.text_value(app).trim().to_owned();
            typed.clear(app);
            if !line.is_empty() {
                self.set_state(app, |state| state.added.push(line));
            }
        });
        let now = app.get(self).now;
        let (h, m, s) = (now.hour(app), now.minute(app), now.second(app));
        let clock = format!("{h:02}:{m:02}:{s:02}");
        let mut lines = vec![
            Text::new(clock).into_widget(),
            CupertinoTextField::new()
                .controller(entry.into_erased())
                .placeholder("Type a line")
                .into_widget(),
            CupertinoButton::filled(Text::new("Add").into_widget(), Some(add)).into_widget(),
        ];
        // What was added shows below the field.
        for line in app.get(self).added.clone() {
            lines.push(Text::new(line).into_widget());
        }
        CupertinoPageScaffold::new(
            Padding::new(EdgeInsetsGeometry::all(24.0)).child(
                Column::new()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .spacing(12.0)
                    .children(lines),
            ),
        )
        .into_widget()
    }
}
