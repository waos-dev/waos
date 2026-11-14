//! The camera's page: the live picture, black under a phone's status bar as a phone's camera is, or what the window says while there is none, over the
//! looks to choose among and the controls. Space takes a photo, the left and right arrows choose
//! the look, and the picture blinks white while a photo is taken.

use std::rc::Rc;
use std::time::Duration;

use waft::ui::{CameraInterruption, CaptureState};
use waft::{
    AnimatedOpacity, App, BuildContext, CallbackShortcuts, CameraSession, Color, ColoredBox,
    Column, CrossAxisAlignment, Entity, ExcludeFocus, Expanded, Focus, IgnorePointer, IntoWidget,
    Listener, MediaQuery, Positioned, ShortcutActivatorRef, SingleActivator, Stack, StackFit,
    StatelessWidget, WidgetRef,
};
use waft_services::LogicalKeyboardKey;
use waft_winui::{Brush, Page, ThemeResources};

use crate::camera_app::CameraApp;
use crate::controls::Controls;
use crate::darkroom::Darkroom;
use crate::live_view::LiveView;
use crate::look_bar::LookBar;
use crate::notice_bar::NoticeBar;
use crate::shooting::take_photo;
use crate::status_message::StatusMessage;
use crate::viewfinder::Viewfinder;

/// What is behind the picture, as a camera's viewfinder is black around it.
const BLACK: Color = Color::from_argb(255, 0, 0, 0);

/// The veil over the picture while a photo is taken: its colour, how much of it shows, and how
/// long it takes to come and go.
const WHITE: Color = Color::from_argb(255, 255, 255, 255);
const BLINK_OPACITY: f64 = 0.5;
const BLINK: Duration = Duration::from_millis(120);

#[derive(Debug)]
pub(crate) struct CameraPage {
    pub(crate) camera: CameraApp,
}

impl StatelessWidget for CameraPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let page = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Expanded::new(self.finder_page(app, context)).into_widget(),
                self.bars_page(app, context),
            ]);
        CallbackShortcuts::new(self.shortcuts(), Focus::new(page).autofocus(true)).into_widget()
    }
}

impl CameraPage {
    /// The finder on a page of its own, black up to the screen's top and sides under a phone's
    /// bars, as a phone's camera is, its picture and notice clear of them. The bars below it keep
    /// the screen's bottom, so its page does not.
    fn finder_page(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let page = Page::new()
            .background(Brush::Solid(BLACK))
            .content(self.finder(app));
        let above_the_bars = MediaQuery::of(app, context)
            .remove_padding(false, false, false, true)
            .remove_view_insets(false, false, false, true);
        MediaQuery::new(above_the_bars, page).into_widget()
    }

    /// The looks and the controls on a page of their own, in Fluent's background down to the
    /// screen's bottom and sides under a phone's bars, clear of them. The finder above keeps the
    /// screen's top, so their page does not.
    fn bars_page(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let background = ThemeResources::of(app, context).solid_background_fill_color_base;
        let page = Page::new()
            .background(Brush::Solid(background))
            .content(self.bars());
        MediaQuery::remove_padding(app, context, false, true, false, false, page).into_widget()
    }

    /// The looks and the controls. None of them takes the keyboard, which stays the page's, so
    /// Space takes a photo whatever was clicked last.
    fn bars(&self) -> WidgetRef {
        let bars = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                LookBar {
                    camera: self.camera.clone(),
                }
                .into_widget(),
                Controls {
                    camera: self.camera.clone(),
                }
                .into_widget(),
            ]);
        ExcludeFocus::new(bars).into_widget()
    }

    /// The keys the page answers: Space takes a photo, and the left and right arrows choose
    /// the look before and after the one chosen.
    fn shortcuts(&self) -> Vec<(ShortcutActivatorRef, Listener)> {
        let on = |key, done: Listener| -> (ShortcutActivatorRef, Listener) {
            (Rc::new(SingleActivator::new(key)), done)
        };
        let camera = self.camera.clone();
        let (before, after) = (
            self.camera.viewfinder.clone(),
            self.camera.viewfinder.clone(),
        );
        vec![
            on(
                LogicalKeyboardKey::SPACE,
                Listener::new(move |app: &mut App| take_photo(app, &camera)),
            ),
            on(
                LogicalKeyboardKey::ARROW_LEFT,
                Listener::new(move |app: &mut App| step_look(app, &before, -1)),
            ),
            on(
                LogicalKeyboardKey::ARROW_RIGHT,
                Listener::new(move |app: &mut App| step_look(app, &after, 1)),
            ),
        ]
    }

    /// The live picture on black, or what is said in its place, with what the window says of a
    /// command over its top.
    fn finder(&self, app: &App) -> WidgetRef {
        let notice = NoticeBar {
            camera: self.camera.clone(),
        };
        Stack::new()
            .fit(StackFit::Expand)
            .children([
                ColoredBox::new(BLACK)
                    .child(self.picture(app))
                    .into_widget(),
                self.blink(app),
                Positioned::new(notice)
                    .left(0.0)
                    .right(0.0)
                    .top(0.0)
                    .into_widget(),
            ])
            .into_widget()
    }

    /// Why the running camera gives no frames for now, where the system gives a reason; none
    /// while it gives them.
    fn interruption(&self, app: &App) -> Option<Option<CameraInterruption>> {
        let running = self.camera.viewfinder.read(app).running()?;
        match CameraSession::state(running.session, app) {
            CaptureState::Interrupted(why) => Some(*why),
            CaptureState::Running | CaptureState::Ended(_) => None,
        }
    }

    /// A white veil over the picture while a photo is being taken, as a shutter blinks.
    fn blink(&self, app: &App) -> WidgetRef {
        let taking = self.camera.viewfinder.read(app).is_taking_photo();
        let veil = if taking { BLINK_OPACITY } else { 0.0 };
        IgnorePointer::new()
            .child(AnimatedOpacity::new(veil, BLINK).child(ColoredBox::new(WHITE)))
            .into_widget()
    }

    fn picture(&self, app: &App) -> WidgetRef {
        if let Some(message) = StatusMessage::of(self.camera.viewfinder.read(app).status()) {
            return message.into_widget();
        }
        if let Darkroom::Shut(why) = self.camera.darkroom.read(app) {
            return StatusMessage::no_drawing(why).into_widget();
        }
        if let Some(why) = self.interruption(app) {
            return StatusMessage::interrupted(why).into_widget();
        }
        LiveView {
            viewfinder: self.camera.viewfinder.clone(),
            darkroom: self.camera.darkroom.clone(),
        }
        .into_widget()
    }
}

/// Chooses the look `by` places after the one chosen.
fn step_look(app: &mut App, viewfinder: &Entity<Viewfinder>, by: isize) {
    viewfinder.update(app, |viewfinder, cx| {
        viewfinder.set_look(viewfinder.look().stepped(by));
        cx.notify();
    });
}
