//! What the user opens with the program, as Waft's app hears it (`EmbedderClient::open_requested`):
//! the WIT's open requests, each file as the path WaOS mounted it at, writable, as a file dialog's
//! pick is answered (decision 6 of the opening proposal).

use std::path::PathBuf;

use waft_embedder::{OpenRequest, PickedFile};
use wasdk::lifecycle;

/// `request` in Waft's terms.
pub(crate) fn framework_open_request(request: lifecycle::OpenRequest) -> OpenRequest {
    OpenRequest {
        files: request
            .files
            .into_iter()
            .map(|path| PickedFile::Path(PathBuf::from(path)))
            .collect(),
        links: request.links,
    }
}
