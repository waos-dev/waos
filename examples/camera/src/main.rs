//! An Waft app that shows a camera's preview: the first camera the system lists, opened at once,
//! which asks the user the first time, then started in its largest format up to 1920 by 1080, with
//! its preview on a composition layer and its photos at the largest size that format takes them
//! at. It prints the cameras the system lists, as `cameras: FaceTime HD Camera (front)`, and again
//! each time one comes or goes; the camera's formats, as `format 1920x1080 at 1-30 fps, photos 1920x1080`, the
//! one it runs in, as `runs in 1920x1080 at 1-30 fps, photos 1920x1080`, and its state now and at
//! each change, as `state: running`, `state: interrupted (another app has the camera)` or, where
//! the system gives no reason, as macOS does not, `state: interrupted`; a button
//! under the preview takes a photo, printing its type and size, as `photo: image/jpeg, 352841
//! bytes`. Where the system has no cameras, or the user does not allow them, the window says so.
//! Asking to close its window quits it.
//!
//! ```text
//! cargo build -p camera --target wasm32-wasip2
//! cargo waos run -p camera
//! ```

use std::rc::Rc;

use waft_cupertino::{CupertinoApp, CupertinoButton, CupertinoPageScaffold};
use waft_embedder::{
    Camera, CameraDevice, CameraError, CameraFacing, CameraFormat, CameraInterruption,
    CameraOutputs, CaptureState, WindowConfig,
};
use waft_foundation::{App, AsyncApp, Entity, Handle, Listenable, Listener};
use waft_widgets::{
    BuildContext, CameraPreview, CameraSession, Center, Column, EdgeInsetsGeometry, Expanded,
    IntoWidget, Padding, StatelessWidget, Text, WidgetRef, available_cameras,
    available_cameras_changed, open_camera, run_app_with,
};

/// The largest picture the camera is started at.
const LARGEST: (f64, f64) = (1920.0, 1080.0);

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        print_cameras(app);
        let pane = app.new_entity(|_cx| CameraPane::Opening);
        open_first_camera(app, &pane);
        let window = WindowConfig {
            title: "Camera".to_owned(),
            size: [800.0, 600.0],
            ..WindowConfig::default()
        };
        let page = CupertinoPageScaffold::new(CameraSide { pane }.into_widget());
        run_app_with(app, CupertinoApp::new().home(page).into_widget(), window);
    })
    .await;
}

/// Where the camera stands.
#[derive(Debug)]
enum CameraPane {
    Opening,
    NoCamera,
    Running(Handle<CameraSession>),
    Failed(CameraError),
}

/// Prints the cameras the system lists now, and again each time one comes or goes.
fn print_cameras(app: &mut App) {
    let _printing = app.spawn(async move |cx| {
        loop {
            // The next change is asked for before the cameras are listed, so none goes unheard.
            let (changed, listing) =
                cx.update(|app| (available_cameras_changed(app), available_cameras(app)));
            println!("cameras: {}", described_cameras(&listing.await));
            let Some(changed) = changed else {
                return;
            };
            changed.await;
        }
    });
}

/// Opens the first camera the system lists, and once the user lets the program use it, starts it
/// with its preview and its photos.
fn open_first_camera(app: &mut App, pane: &Entity<CameraPane>) {
    let listing = available_cameras(app);
    let pane = pane.clone();
    let _opening = app.spawn(async move |cx| {
        let next = match listing.await.into_iter().next() {
            Some(camera) => run_camera(cx, &camera).await,
            None => CameraPane::NoCamera,
        };
        cx.update(|app| show(app, &pane, next));
    });
}

/// Opens `camera` and starts it: the pane that shows it running, or what kept it from running.
async fn run_camera(cx: &AsyncApp, camera: &Camera) -> CameraPane {
    let Some(opening) = cx.update(|app| open_camera(app, camera)) else {
        return CameraPane::NoCamera;
    };
    let started = match opening.await {
        Ok(device) => start(cx, &*device).await,
        Err(error) => Err(error),
    };
    match started {
        Ok(session) => CameraPane::Running(session),
        Err(error) => CameraPane::Failed(error),
    }
}

fn show(app: &mut App, pane: &Entity<CameraPane>, next: CameraPane) {
    pane.update(app, |pane, cx| {
        *pane = next;
        cx.notify();
    });
}

/// Starts `device` in its preferred format, its preview on a composition layer and its photos at
/// that format's largest size, printing its formats, the one it runs in and its state as it
/// changes.
async fn start(
    cx: &AsyncApp,
    device: &dyn CameraDevice,
) -> Result<Handle<CameraSession>, CameraError> {
    let formats = device.formats();
    for format in &formats {
        println!("format {}", described(format));
    }
    let format = preferred(&formats)
        .ok_or_else(|| CameraError::Failed("the camera lists no format".to_owned()))?;
    let starting = cx.update(|app| {
        let mut outputs = CameraOutputs::new();
        if let Some(compositor) = app.platform().compositor() {
            outputs = outputs.preview(compositor.make_layer(false));
        }
        if let Some(size) = largest_photo_size(format) {
            outputs = outputs.photos(size);
        }
        CameraSession::start(app, device, format, outputs)
    });
    let session = starting.await?;
    cx.update(|app| {
        println!("runs in {}", described(CameraSession::format(session, app)));
        print_states(app, session);
    });
    Ok(session)
}

/// The largest format up to [`LARGEST`], the fastest of those that size; else the first.
fn preferred(formats: &[CameraFormat]) -> Option<&CameraFormat> {
    let fits = |format: &&CameraFormat| {
        format.size.width() <= LARGEST.0 && format.size.height() <= LARGEST.1
    };
    formats
        .iter()
        .filter(fits)
        .max_by(|a, b| {
            area(a.size)
                .total_cmp(&area(b.size))
                .then(a.max_frame_rate.total_cmp(&b.max_frame_rate))
        })
        .or(formats.first())
}

fn largest_photo_size(format: &CameraFormat) -> Option<waft_embedder::Size> {
    format
        .photo_sizes
        .iter()
        .copied()
        .max_by(|a, b| area(*a).total_cmp(&area(*b)))
}

fn area(size: waft_embedder::Size) -> f64 {
    size.width() * size.height()
}

/// Prints `session`'s state now and each time it changes.
fn print_states(app: &mut App, session: Handle<CameraSession>) {
    let print = Listener::new(move |app| {
        println!(
            "state: {}",
            described_state(CameraSession::state(session, app))
        );
    });
    print.call(app);
    let listenable: Rc<dyn Listenable> = Rc::new(session);
    listenable.add_listener(app, print);
}

/// Takes a photo of `session`, printing its type and size once the system has encoded it.
fn take_photo(app: &mut App, session: Handle<CameraSession>) {
    let photo = CameraSession::take_photo(session, app);
    let _taking = app.spawn(async move |_cx| match photo.await {
        Ok(photo) => println!("photo: {}, {} bytes", photo.mime_type, photo.bytes.len()),
        Err(error) => println!("photo failed: {error}"),
    });
}

fn described(format: &CameraFormat) -> String {
    let photos: Vec<String> = format.photo_sizes.iter().map(|size| sized(*size)).collect();
    format!(
        "{} at {}-{} fps, photos {}",
        sized(format.size),
        format.min_frame_rate,
        format.max_frame_rate,
        photos.join(" ")
    )
}

/// Each camera's name, with which way it faces; `none` for none.
fn described_cameras(cameras: &[Camera]) -> String {
    if cameras.is_empty() {
        return "none".to_owned();
    }
    let described: Vec<String> = cameras
        .iter()
        .map(|camera| format!("{} ({})", camera.name, facing_words(camera.facing)))
        .collect();
    described.join(", ")
}

fn facing_words(facing: CameraFacing) -> &'static str {
    match facing {
        CameraFacing::Front => "front",
        CameraFacing::Back => "back",
        CameraFacing::External => "external",
    }
}

fn sized(size: waft_embedder::Size) -> String {
    format!("{}x{}", size.width(), size.height())
}

fn described_state(state: &CaptureState) -> String {
    match state {
        CaptureState::Running => "running".to_owned(),
        CaptureState::Interrupted(Some(why)) => {
            format!("interrupted ({})", interruption_words(*why))
        }
        CaptureState::Interrupted(None) => "interrupted".to_owned(),
        CaptureState::Ended(why) => format!("ended ({why})"),
    }
}

fn interruption_words(interruption: CameraInterruption) -> &'static str {
    match interruption {
        CameraInterruption::InUseByAnotherApp => "another app has the camera",
        CameraInterruption::Background => "in the background",
        CameraInterruption::SystemPressure => "the system is too hot or too busy",
    }
}

/// The camera's preview with the photo button once it runs, or what keeps it from running.
#[derive(Debug)]
struct CameraSide {
    pane: Entity<CameraPane>,
}

impl StatelessWidget for CameraSide {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        match self.pane.read(app) {
            CameraPane::Opening => label("Opening the camera…"),
            CameraPane::NoCamera => label("This system has no cameras"),
            CameraPane::Failed(error) => label(&format!("No camera: {error}")),
            CameraPane::Running(session) => preview_with_button(*session),
        }
    }
}

/// `session`'s preview, with the button that takes a photo under it.
fn preview_with_button(session: Handle<CameraSession>) -> WidgetRef {
    let press = Listener::new(move |app| take_photo(app, session));
    let button = CupertinoButton::filled(Text::new("Take a photo").into_widget(), Some(press));
    Column::new()
        .children([
            Expanded::new(Center::new().child(CameraPreview::new(session))).into_widget(),
            Padding::new(EdgeInsetsGeometry::all(16.0))
                .child(Center::new().child(button))
                .into_widget(),
        ])
        .into_widget()
}

fn label(text: &str) -> WidgetRef {
    Center::new().child(Text::new(text)).into_widget()
}
