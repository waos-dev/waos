//! The photos kept: one shown large over a strip of them all, the newest first, with its name,
//! when it was taken, and what is done to it above it: exported, a copy where the user picks, or deleted, after asking.
//! A tap in the strip, or the left and right arrows, shows another; Back, or Esc, returns to the
//! camera, as deleting the last photo does.

use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use waft::ui::Image;
use waft::{
    App, Axis, BoxFit, BuildContext, CallbackShortcuts, Column, CrossAxisAlignment,
    EdgeInsetsGeometry, Expanded, Focus, GestureDetector, Handle, ImageInfo, IntoWidget, ListView,
    Listener, Navigator, Padding, PageRouteBuilder, RawImage, Row, SingleActivator, Size, SizedBox,
    State, StateData, StatefulWidget, Text, WidgetRef,
};
use waft_services::LogicalKeyboardKey;
use waft_winui::{
    Brush, Button, ButtonStyle, ContentDialog, ContentDialogButton, ContentDialogResult,
    InfoBarSeverity, Page, TextBlockStyle, ThemeResources, ToolTip, ToolTipService,
};

use crate::camera_app::CameraApp;
use crate::dates;
use crate::exporting::export;
use crate::icons::{self, icon};
use crate::library::name_of;
use crate::notice_bar::NoticeBar;
use crate::photo_tile::photo_tile;
use crate::pictures::picture_of;
use crate::roll;

/// The box the photo shown is decoded to fit, in pixels: a large window on a dense screen.
const LARGE: Size = Size::new(2880.0, 2880.0);

/// A strip tile's width across, and the room around the strip, in logical points.
const TILE: f64 = 64.0;
const STRIP_MARGIN: f64 = 12.0;

/// Opens the photos over the camera's page, showing the one at `shown`.
pub(crate) fn open(app: &mut App, context: BuildContext, camera: &CameraApp, shown: PathBuf) {
    camera.dismiss_notice(app);
    let camera = camera.clone();
    let page = Rc::new(move |_app: &mut App, _context: BuildContext, _, _| {
        PhotosPage {
            camera: camera.clone(),
            first: shown.clone(),
        }
        .into_widget()
    });
    let route = PageRouteBuilder::new(app, page);
    let _closed = Navigator::push(app, context, route.into_erased());
}

struct PhotosPage {
    camera: CameraApp,
    /// The photo shown as the page opens.
    first: PathBuf,
}

impl fmt::Debug for PhotosPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhotosPage")
            .field("first", &self.first)
            .finish_non_exhaustive()
    }
}

impl StatefulWidget for PhotosPage {
    type State = PhotosPageState;

    fn create_state(&self) -> PhotosPageState {
        PhotosPageState {
            state: StateData::new(),
            shown: self.first.clone(),
            large: None,
        }
    }
}

struct PhotosPageState {
    state: StateData<PhotosPage>,
    /// The photo shown large.
    shown: PathBuf,
    /// The photo decoded large, and whose it is; its thumbnail shows until it is the shown one's.
    large: Option<(PathBuf, Image)>,
}

impl PhotosPageState {
    fn camera(self: Handle<Self>, app: &App) -> CameraApp {
        self.widget(app).camera.clone()
    }

    /// Shows the photo at `path`, decoding it large.
    fn show(self: Handle<Self>, app: &mut App, path: PathBuf) {
        self.set_state(app, |state| state.shown = path.clone());
        app.spawn(async move |cx| {
            let Some(image) = picture_of(cx, &path, Some(LARGE)).await else {
                return;
            };
            cx.update(|app| {
                if self.mounted(app) && app.get(self).shown == path {
                    self.set_state(app, |state| state.large = Some((path, image)));
                }
            });
        });
    }

    /// Shows the photo `by` places after the one shown in the strip, where there is one.
    fn step(self: Handle<Self>, app: &mut App, by: isize) {
        let roll = self.camera(app).roll;
        let roll = roll.read(app);
        let next = roll
            .position(&app.get(self).shown)
            .and_then(|at| at.checked_add_signed(by))
            .and_then(|at| roll.photos().get(at))
            .map(|photo| photo.path.clone());
        if let Some(next) = next {
            self.show(app, next);
        }
    }

    fn export_shown(self: Handle<Self>, app: &mut App) {
        let (camera, shown) = (self.camera(app), app.get(self).shown.clone());
        export(app, &camera, &shown);
    }

    /// Asks whether to delete the photo shown, and deletes it at a yes.
    fn ask_to_delete_shown(self: Handle<Self>, app: &mut App) {
        let shown = app.get(self).shown.clone();
        let dialog = ContentDialog::new(app);
        dialog.title(app, Text::new(format!("Delete “{}”?", name_of(&shown))));
        dialog.content(app, Text::new("The photo is deleted for good."));
        dialog.primary_button_text(app, "Delete");
        dialog.close_button_text(app, "Cancel");
        dialog.default_button(app, ContentDialogButton::Close);
        let Ok(answered) = dialog.show_async(app, self.context(app)) else {
            return dialog.dispose(app);
        };
        app.spawn(async move |cx| {
            let answer = answered.await;
            cx.update(|app| {
                dialog.dispose(app);
                if answer == ContentDialogResult::Primary && self.mounted(app) {
                    self.delete(app, &shown);
                }
            });
        });
    }

    /// Deletes the photo at `path`, then shows the one that took its place in the strip, or the
    /// camera once none is left.
    fn delete(self: Handle<Self>, app: &mut App, path: &Path) {
        let camera = self.camera(app);
        let at = camera.roll.read(app).position(path).unwrap_or(0);
        if let Err(error) = roll::delete(app, &camera.roll, path) {
            let said = format!("“{}” could not be deleted: {error}.", name_of(path));
            return camera.say(app, InfoBarSeverity::Error, said);
        }
        let photos = camera.roll.read(app).photos();
        let next = photos
            .get(at)
            .or(photos.last())
            .map(|photo| photo.path.clone());
        match next {
            Some(next) => self.show(app, next),
            None => Navigator::pop(app, self.context(app), None),
        }
    }

    /// The keys the page answers: Esc, the arrows, and Delete.
    fn shortcuts(self: Handle<Self>) -> Vec<(waft::ShortcutActivatorRef, Listener)> {
        let on = |key, done: Listener| -> (waft::ShortcutActivatorRef, Listener) {
            (Rc::new(SingleActivator::new(key)), done)
        };
        vec![
            on(
                LogicalKeyboardKey::ESCAPE,
                Listener::new(move |app| Navigator::pop(app, self.context(app), None)),
            ),
            on(
                LogicalKeyboardKey::ARROW_LEFT,
                Listener::new(move |app| self.step(app, -1)),
            ),
            on(
                LogicalKeyboardKey::ARROW_RIGHT,
                Listener::new(move |app| self.step(app, 1)),
            ),
            on(
                LogicalKeyboardKey::DELETE,
                Listener::new(move |app| self.ask_to_delete_shown(app)),
            ),
            on(
                LogicalKeyboardKey::BACKSPACE,
                Listener::new(move |app| self.ask_to_delete_shown(app)),
            ),
        ]
    }

    /// Back to the camera on the left, the shown photo's name over when it was taken, and what is
    /// done to it on the right.
    fn bar(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let r = ThemeResources::of(app, context);
        let name = Text::new(name_of(&app.get(self).shown))
            .style(TextBlockStyle::BodyStrong.text_style(&r, r.text_fill_color_primary));
        let mut identity = vec![name.into_widget()];
        if let Some(taken) = self.shown_taken(app) {
            let line = Text::new(dates::in_full(taken, dates::users_offset))
                .style(TextBlockStyle::Caption.text_style(&r, r.text_fill_color_secondary));
            identity.push(line.into_widget());
        }
        let identity = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children(identity);
        let back = labelled_button(
            icons::BACK,
            "Camera",
            Listener::new(move |app| Navigator::pop(app, context, None)),
        );
        let export = labelled_button(
            icons::EXPORT,
            "Export…",
            Listener::new(move |app| self.export_shown(app)),
        );
        let delete = Button::new(
            icon(icons::DELETE, 18.0),
            Listener::new(move |app| self.ask_to_delete_shown(app)),
        )
        .style(ButtonStyle::Subtle);
        let row = Row::new().children([
            back,
            SizedBox::new().width(12.0).into_widget(),
            Expanded::new(identity).into_widget(),
            export,
            SizedBox::new().width(4.0).into_widget(),
            ToolTipService::new(delete, ToolTip::text("Delete")).into_widget(),
        ]);
        Padding::new(EdgeInsetsGeometry::from_ltrb(8.0, 8.0, 8.0, 8.0))
            .child(row)
            .into_widget()
    }

    /// When the photo shown was taken; none where the roll does not know.
    fn shown_taken(self: Handle<Self>, app: &App) -> Option<std::time::SystemTime> {
        let roll = self.camera(app).roll.read(app);
        let at = roll.position(&app.get(self).shown)?;
        roll.photos()[at].taken
    }

    /// The photo shown, whole, in the room there is: decoded large, or its thumbnail until then.
    fn picture(self: Handle<Self>, app: &App) -> WidgetRef {
        let state = app.get(self);
        let large = state
            .large
            .as_ref()
            .filter(|(path, _)| *path == state.shown)
            .map(|(_, image)| image.clone());
        let thumbnail = || {
            let roll = self.widget(app).camera.roll.read(app);
            let at = roll.position(&state.shown)?;
            roll.photos()[at].thumbnail.clone()
        };
        let Some(image) = large.or_else(thumbnail) else {
            return SizedBox::expand().into_widget();
        };
        let shown = RawImage::new(Some(ImageInfo::new(image))).fit(BoxFit::Contain);
        Padding::new(EdgeInsetsGeometry::from_ltrb(16.0, 0.0, 16.0, 4.0))
            .child(SizedBox::expand().child(shown))
            .into_widget()
    }

    /// Every photo kept as a tile, the newest first, the one shown ringed.
    fn strip(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let ring = ThemeResources::of(app, context).accent_text_fill_color_primary_brush;
        let roll = self.camera(app).roll;
        let count = roll.read(app).photos().len();
        let tiles = ListView::builder(move |app, _context, index| {
            let photo = roll.read(app).photos().get(usize::try_from(index).ok()?)?;
            let ringed = (photo.path == app.get(self).shown).then_some(ring);
            let tile = photo_tile(photo.thumbnail.clone(), TILE, ringed);
            let path = photo.path.clone();
            let show = Listener::new(move |app: &mut App| self.show(app, path.clone()));
            Some(
                GestureDetector::new()
                    .on_tap(show)
                    .child(tile)
                    .into_widget(),
            )
        })
        .item_count(i32::try_from(count).unwrap_or(i32::MAX))
        .build()
        .scroll_direction(Axis::Horizontal)
        .padding(EdgeInsetsGeometry::from_ltrb(
            STRIP_MARGIN,
            0.0,
            STRIP_MARGIN,
            0.0,
        ));
        Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 8.0, 0.0, STRIP_MARGIN))
            .child(SizedBox::new().height(TILE).child(tiles))
            .into_widget()
    }
}

impl State for PhotosPageState {
    type Widget = PhotosPage;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let first = app.get(self).shown.clone();
        self.show(app, first);
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let background = ThemeResources::of(app, context).solid_background_fill_color_base;
        let notice = NoticeBar {
            camera: self.camera(app),
        };
        let page = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                self.bar(app, context),
                notice.into_widget(),
                Expanded::new(self.picture(app)).into_widget(),
                self.strip(app, context),
            ]);
        let page = Page::new()
            .background(Brush::Solid(background))
            .content(page);
        CallbackShortcuts::new(self.shortcuts(), Focus::new(page).autofocus(true)).into_widget()
    }
}

/// A subtle Fluent button showing `glyph` before `label`.
fn labelled_button(glyph: char, label: &str, click: Listener) -> WidgetRef {
    let content = Row::new()
        .main_axis_size(waft::MainAxisSize::Min)
        .children([
            icon(glyph, 16.0).into_widget(),
            SizedBox::new().width(8.0).into_widget(),
            Text::new(label.to_owned()).into_widget(),
        ]);
    Button::new(content, click)
        .style(ButtonStyle::Subtle)
        .into_widget()
}
