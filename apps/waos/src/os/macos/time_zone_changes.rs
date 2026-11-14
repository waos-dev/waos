//! Each change of the user's time zone, as macOS announces it: Foundation's
//! `NSSystemTimeZoneDidChangeNotification`, heard on the main queue.

// Foundation's notification center takes and lets go of an observer through calls its bindings
// declare unsafe.
#![allow(unsafe_code)]

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSOperationQueue, NSSystemTimeZoneDidChangeNotification,
};

/// What hears the changes, for as long as it is held.
pub(crate) struct TimeZoneChanges(Retained<ProtocolObject<dyn NSObjectProtocol>>);

impl Drop for TimeZoneChanges {
    fn drop(&mut self) {
        // SAFETY: the observer is the one the center handed out for this registration.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self.0.as_ref()) };
    }
}

/// Calls `changed` on the main thread each time the user's time zone changes.
pub(crate) fn watch_time_zone(
    changed: impl Fn() + Send + Sync + 'static,
) -> Option<TimeZoneChanges> {
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| changed());
    let queue = NSOperationQueue::mainQueue();
    // SAFETY: no object is named, the main queue has no threading requirement of its own, and the
    // block is `Send + Sync`, so it may run wherever the center calls it.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSSystemTimeZoneDidChangeNotification),
            None,
            Some(&queue),
            &block,
        )
    };
    Some(TimeZoneChanges(observer))
}
