//! Taking a photo: the system takes and encodes it, at the size the camera was started with; it
//! is developed in the look the viewfinder shows as the shutter is pressed, and kept as the
//! roll's newest. One photo at a time: the shutter does nothing until the last is kept.

use waft::ui::PhotoFuture;
use waft::{App, AsyncApp, CameraSession};
use waft_winui::InfoBarSeverity;

use crate::camera_app::CameraApp;
use crate::developing::developed;
use crate::looks::Look;
use crate::roll;

/// Takes a photo where the camera runs and none is being taken, and says so where it fails.
pub(crate) fn take_photo(app: &mut App, camera: &CameraApp) {
    let Some((taking, look)) = begin(app, camera) else {
        return;
    };
    camera.dismiss_notice(app);
    let camera = camera.clone();
    app.spawn(async move |cx| {
        let kept = keep_taken(cx, &camera, taking, look).await;
        cx.update(|app| finish(app, &camera, kept));
    });
}

/// Has the system take a photo, where the viewfinder runs and takes none: the photo on its way,
/// and the look to develop it in.
fn begin(app: &mut App, camera: &CameraApp) -> Option<(PhotoFuture, Look)> {
    let viewfinder = camera.viewfinder.read(app);
    if viewfinder.is_taking_photo() {
        return None;
    }
    let (session, look) = (viewfinder.running()?.session.handle(), viewfinder.look());
    let taking = CameraSession::take_photo(session, app);
    camera.viewfinder.update(app, |viewfinder, cx| {
        viewfinder.set_taking_photo(true);
        cx.notify();
    });
    Some((taking, look))
}

/// Waits for the photo, develops it in `look` and keeps it; why not, worded for the user.
async fn keep_taken(
    cx: &AsyncApp,
    camera: &CameraApp,
    taking: PhotoFuture,
    look: Look,
) -> Result<(), String> {
    let photo = taking
        .await
        .map_err(|error| format!("The photo was not taken: {error}."))?;
    let photo = developed(cx, &camera.darkroom, photo, look).await?;
    cx.update(|app| roll::keep(app, &camera.roll, &photo.encoded, &photo.extension))
        .map_err(|error| format!("The photo could not be kept: {error}."))
}

fn finish(app: &mut App, camera: &CameraApp, kept: Result<(), String>) {
    camera.viewfinder.update(app, |viewfinder, cx| {
        viewfinder.set_taking_photo(false);
        cx.notify();
    });
    if let Err(why) = kept {
        camera.say(app, InfoBarSeverity::Error, why);
    }
}
