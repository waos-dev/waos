// Waft's `StatefulWidget::State` takes `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use std::rc::Rc;
use std::time::Duration;

use waft::*;
use waft_winui::*;

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Asks for the app's window, and gives WinUI's controls their icons.
fn start(app: &mut App) {
    let window = WindowConfig {
        size: [1080.0, 780.0],
        ..WindowConfig::default()
    };
    run_app_with(app, widgets_app(), window);
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// The app's pages, in Fluent's accent, `Home` the first.
fn widgets_app() -> WidgetRef {
    WidgetsApp::new(AccentPalette::default().base)
        // Hides Flutter's DEBUG ribbon: `debug_show_checked_mode_banner(false)`.
        .debug_show_checked_mode_banner(false)
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            route.into_erased()
        })
        .home(Home)
        .into_widget()
}

/// A typed line, the lines added below it, and a clock.
#[derive(Debug)]
struct Home;

/// What survives a rebuild: the box's controller, the lines added, and the time.
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
                cx.update(|app| {
                    Timer::new(
                        app,
                        Duration::from_secs(1),
                        Listener::new(move |app| wait.complete(app, ())),
                    )
                });
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

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let background = ThemeResources::of(app, context).solid_background_fill_color_base;
        let entry = app.get(self).entry.clone().expect("made in init_state");
        let typed = entry.clone();
        // Add appends the box's line to the list, then empties the box for the next one.
        let add = Listener::new(move |app| {
            let line = typed.text_value(app).trim().to_owned();
            if line.is_empty() {
                return;
            }
            typed.clear(app);
            self.set_state(app, |state| state.added.push(line));
        });
        let now = app.get(self).now;
        let (h, m, s) = (now.hour(app), now.minute(app), now.second(app));
        let clock = format!("{h:02}:{m:02}:{s:02}");
        let mut lines = vec![
            Text::new(clock).into_widget(),
            TextBox::new(entry.into_erased())
                .placeholder_text("Type a line")
                .into_widget(),
            Button::text("Add", add).into_widget(),
        ];
        // What was added shows below the box.
        for line in app.get(self).added.clone() {
            lines.push(Text::new(line).into_widget());
        }
        ColoredBox::new(background)
            .child(
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
