//! The camera's live picture in the window: at each frame the window draws, the newest frame the
//! camera handed out is drawn through the look chosen and shown, on a composition layer placed
//! where the widget is, kept in the picture's own shape. The picture is drawn at the layer's size
//! in the screen's pixels, as the system shows a layer's buffer at its own pixels on every system.

use std::cell::Cell;
use std::rc::Rc;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use waft::{
    App, AspectRatio, BoxConstraints, BuildContext, CameraSession, Center, CompositionLayerView,
    Entity, FrameCallback, Handle, IntoWidget, LayoutBuilder, MediaQuery,
    SingleTickerProviderStateMixin, SingleTickerProviderStateMixinData, SizedBox, State, StateData,
    StatefulWidget, Ticker, WidgetRef,
};

use crate::darkroom::Darkroom;
use crate::live_picture::LivePicture;
use crate::look_painter::Shown;
use crate::viewfinder::Viewfinder;

pub(crate) struct LiveView {
    pub(crate) viewfinder: Entity<Viewfinder>,
    pub(crate) darkroom: Entity<Darkroom>,
}

impl fmt::Debug for LiveView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveView").finish_non_exhaustive()
    }
}

impl StatefulWidget for LiveView {
    type State = LiveViewState;

    fn create_state(&self) -> LiveViewState {
        LiveViewState {
            state: StateData::new(),
            single_ticker_provider: SingleTickerProviderStateMixinData::default(),
            ticker: None,
            picture: None,
            layer_pixels: Rc::default(),
        }
    }
}

pub(crate) struct LiveViewState {
    state: StateData<LiveView>,
    single_ticker_provider: SingleTickerProviderStateMixinData,
    /// Shows the newest frame at each frame the window draws, while the widget is in sight.
    ticker: Option<Handle<Ticker>>,
    /// None where the system has no compositor to show it.
    picture: Option<LivePicture>,
    /// The layer's size in the screen's pixels as it was last laid out, which the picture is
    /// drawn at; none before.
    layer_pixels: Rc<Cell<Option<[u32; 2]>>>,
}

impl LiveViewState {
    /// Draws the camera's newest frame through the look and shows it, where there is a new one
    /// and the darkroom is open.
    fn show_newest_frame(self: Handle<Self>, app: &mut App) {
        let widget = self.widget(app);
        let Some(painter) = widget.darkroom.read(app).painter() else {
            return;
        };
        let viewfinder = widget.viewfinder.read(app);
        let Some(running) = viewfinder.running() else {
            return;
        };
        let grade = viewfinder.look().grade();
        let frames = Arc::clone(&running.frames);
        let platform = app.platform();
        let Some(frame) = frames.frame_for(platform.now()) else {
            return;
        };
        let Some(layer_pixels) = app.get(self).layer_pixels.get() else {
            return;
        };
        let Some(picture) = app.get_mut(self).picture.as_mut() else {
            return;
        };
        let shown = Shown {
            rotation: frame.rotation(),
            flip: frame.flip(),
        };
        let frame = frame.hardware_buffer();
        let drawn = picture.show(
            &*platform,
            &painter,
            frame.as_ref(),
            layer_pixels,
            grade,
            shown,
        );
        if let Err(why) = drawn {
            tracing::warn!("a frame of the camera's is not shown: {why}");
        }
    }

    /// The picture's width over its height: the format's the camera runs in, turned as what it
    /// captures is.
    fn aspect_ratio(self: Handle<Self>, app: &App) -> Option<f64> {
        let running = self.widget(app).viewfinder.read(app).running()?;
        let size = CameraSession::format(running.session, app).size;
        let shown = Shown {
            rotation: CameraSession::video_rotation_angle_for_horizon_level_capture(
                running.session,
                app,
            ),
            flip: false,
        };
        let [width, height] = shown.size_of([size.width() as u32, size.height() as u32]);
        (height > 0).then(|| f64::from(width) / f64::from(height))
    }
}

impl SingleTickerProviderStateMixin for LiveViewState {
    fn single_ticker_provider_data(
        self: Handle<Self>,
        app: &App,
    ) -> &SingleTickerProviderStateMixinData {
        &app.get(self).single_ticker_provider
    }

    fn single_ticker_provider_data_mut(
        self: Handle<Self>,
        app: &mut App,
    ) -> &mut SingleTickerProviderStateMixinData {
        &mut app.get_mut(self).single_ticker_provider
    }
}

impl State for LiveViewState {
    type Widget = LiveView;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let picture = LivePicture::new(&*app.platform());
        let tick = FrameCallback::new(move |app: &mut App, _elapsed: Duration| {
            self.show_newest_frame(app);
        });
        let ticker = SingleTickerProviderStateMixin::create_ticker(self, app, tick);
        let _ticking = ticker.start(app);
        let state = app.get_mut(self);
        state.picture = picture;
        state.ticker = Some(ticker);
    }

    fn activate(self: Handle<Self>, app: &mut App) {
        SingleTickerProviderStateMixin::activate(self, app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(ticker) = app.get_mut(self).ticker.take() {
            ticker.dispose(app);
        }
        SingleTickerProviderStateMixin::dispose(self, app);
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let layer = app.get(self).picture.as_ref().map(LivePicture::layer);
        let pixel_ratio = MediaQuery::device_pixel_ratio_of(app, context);
        let layer_pixels = Rc::clone(&app.get(self).layer_pixels);
        match (layer, self.aspect_ratio(app)) {
            (Some(layer), Some(aspect_ratio)) => {
                LayoutBuilder::new(move |_app, _context, constraints| {
                    let box_size = largest_at(aspect_ratio, constraints);
                    layer_pixels.set(Some(
                        box_size.map(|side| (side * pixel_ratio).round() as u32),
                    ));
                    Center::new()
                        .child(
                            AspectRatio::new(aspect_ratio)
                                .child(CompositionLayerView::new(Arc::clone(&layer))),
                        )
                        .into_widget()
                })
                .into_widget()
            }
            _ => SizedBox::shrink().into_widget(),
        }
    }
}

/// The largest box of `aspect_ratio`, width over height, that `constraints` let be, in logical
/// pixels, as an `AspectRatio` lays itself out.
fn largest_at(aspect_ratio: f64, constraints: BoxConstraints) -> [f64; 2] {
    let width = constraints
        .max_width
        .min(constraints.max_height * aspect_ratio);
    [width, width / aspect_ratio]
}
