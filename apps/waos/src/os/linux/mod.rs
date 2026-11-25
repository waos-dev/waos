//! What WaOS asks of Linux itself: the dev server announced on the local network, through Avahi or
//! WaOS's own responder (`dev_announcement`).

mod dev_announcement;

pub(crate) use dev_announcement::{DevServerAnnouncement, announce_dev_server};
