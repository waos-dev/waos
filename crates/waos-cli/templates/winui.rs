//! A new WaOS app in WinUI's look, which `cargo waos run` runs. The docs and guides are at
//! https://waos.app/docs.
// Waft's `StatefulWidget::State` takes `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use std::time::Duration;

use waft::*;
use waft_winui::*;

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Asks for the app's window, keeps the app's data in an entity, and starts the clock.
fn start(app: &mut App) {
    let notes = app.new_entity(|_| Notes {
        added: Vec::new(),
        now: DateTime::now(),
    });
    // The clock: a periodic timer reads the time into the entity once a second, and the notify
    // rebuilds the page, which reads the entity in its build.
    let clock = notes.clone();
    Timer::periodic(app, Duration::from_secs(1), move |app, _each| {
        clock.update(app, |notes, cx| {
            notes.now = DateTime::now();
            cx.notify();
        })
    });
    let window = WindowConfig {
        size: [1080.0, 780.0],
        ..WindowConfig::default()
    };
    run_app_with(app, widgets_app(notes), window);
}

/// The app in Fluent's look. `FluentApp` brings WinUI's theme, its icon font and its page
/// entrance, so nothing here installs them by hand.
fn widgets_app(notes: Entity<Notes>) -> WidgetRef {
    FluentApp::new()
        // Hides Flutter's DEBUG ribbon: `debug_show_checked_mode_banner(false)`.
        .debug_show_checked_mode_banner(false)
        .home(Home { notes })
        .into_widget()
}

/// The app's data, kept in an entity: what the page shows, rebuilt when it notifies.
struct Notes {
    /// Each line Add appended, oldest first.
    added: Vec<String>,
    /// The clock's time, read again each second.
    now: DateTime,
}

/// The page: the clock, the box with its Add, and the lines added below. A `StatelessWidget`
/// that reads the entity in `build`, so the page is rebuilt whenever the entity notifies.
#[derive(Debug)]
struct Home {
    notes: Entity<Notes>,
}

impl StatelessWidget for Home {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let background = ThemeResources::of(app, context).solid_background_fill_color_base;
        let (added, now) = {
            let notes = self.notes.read(app);
            (notes.added.clone(), notes.now)
        };
        let (h, m, s) = (now.hour(app), now.minute(app), now.second(app));
        let mut lines = vec![
            Text::new(format!("{h:02}:{m:02}:{s:02}")).into_widget(),
            Entry {
                notes: self.notes.clone(),
            }
            .into_widget(),
        ];
        // What was added shows below the box.
        for line in added {
            lines.push(Text::new(line).into_widget());
        }
        ColoredBox::new(background)
            .child(
                Padding::new(EdgeInsets::all(24.0)).child(
                    Column::new()
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .spacing(12.0)
                        .children(lines),
                ),
            )
            .into_widget()
    }
}

/// The box and its Add button. The controller is a Flutter object, a `Ref`, kept in a `State`:
/// the only one in this app, since nothing else here is widget-local.
#[derive(Debug)]
struct Entry {
    notes: Entity<Notes>,
}

struct EntryState {
    state: StateData<Entry>,
    controller: Option<Ref<TextEditingController>>,
}

impl StatefulWidget for Entry {
    type State = EntryState;

    fn create_state(&self) -> EntryState {
        EntryState {
            state: StateData::new(),
            controller: None,
        }
    }
}

impl State for EntryState {
    type Widget = Entry;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).controller = Some(TextEditingController::new(app));
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(controller) = app.get_mut(self).controller.take() {
            controller.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let controller = app
            .get(self)
            .controller
            .clone()
            .expect("made in init_state");
        let typed = controller.clone();
        let notes = self.widget(app).notes.clone();
        // Add puts the box's line in the app's data, then empties the box for the next one.
        let add = Listener::new(move |app| {
            let line = typed.text_value(app).trim().to_owned();
            if line.is_empty() {
                return;
            }
            typed.clear(app);
            notes.update(app, |notes, cx| {
                notes.added.push(line);
                cx.notify();
            });
        });
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .spacing(8.0)
            .children([
                TextBox::new(controller.into_erased())
                    .placeholder_text("Type a line")
                    .into_widget(),
                Button::text("Add", add).into_widget(),
            ])
            .into_widget()
    }
}
