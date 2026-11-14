//! Waft's opener over `wasdk:platform/opener`, one to one: a link, a file and a file shown in the
//! system's file manager, each a call, a file named by its path inside the program. Whether a link
//! can be opened is not asked: the WIT does not ask it (decision 2 of the opening proposal), and
//! a program learns no app opens a link when opening it fails.

use std::path::Path;
use std::rc::Rc;

use waft_embedder::{OpenError, OpenFuture, Opener};
use wasdk::opener as platform;

/// The platform's opener.
pub(crate) struct WasdkOpener(Rc<platform::Opener>);

impl WasdkOpener {
    /// The platform's opener, where it has one.
    pub(crate) fn get() -> Option<WasdkOpener> {
        platform::get_opener().map(|opener| WasdkOpener(Rc::new(opener)))
    }
}

impl Opener for WasdkOpener {
    fn open_link(&self, link: &str) -> OpenFuture {
        let (opener, link) = (Rc::clone(&self.0), link.to_owned());
        Box::pin(async move { opener.open_link(link).await.map_err(framework_error) })
    }

    fn open_file(&self, path: &Path) -> OpenFuture {
        let (opener, path) = (Rc::clone(&self.0), path_string(path));
        Box::pin(async move { opener.open_file(path).await.map_err(framework_error) })
    }

    fn reveal_file(&self, path: &Path) -> OpenFuture {
        let (opener, path) = (Rc::clone(&self.0), path_string(path));
        Box::pin(async move { opener.reveal_file(path).await.map_err(framework_error) })
    }

    /// None: the platform does not say.
    fn can_open_link(&self, _link: &str) -> Option<bool> {
        None
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Why nothing opened, in Waft's terms, which have no refusal of their own: a scheme the user did
/// not allow the program fails with why.
fn framework_error(error: platform::OpenError) -> OpenError {
    match error {
        platform::OpenError::Denied => OpenError::Failed(
            "the user does not let the program open links of this kind".to_owned(),
        ),
        platform::OpenError::NoApp => OpenError::NoApp,
        platform::OpenError::Failed(reason) => OpenError::Failed(reason),
    }
}
