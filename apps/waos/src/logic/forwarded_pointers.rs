//! The device numbers of a stream of pointer reports WaOS forwards into its own framework from
//! another process's host, as a prompt's dialog in a program's app reports them: each device of the
//! stream (its mouse, its trackpad, each touch from landing to lifting) goes by a random 64-bit
//! number of its own, drawn as the device first reports and let go once it is removed, as Waft's
//! `ViewEmbedder::send_pointer_reports` asks of an app that forwards reports (native-surfaces
//! decision 36): the other host numbers its devices as WaOS's own does, 0, 1, 2 upward, and Waft's
//! converter keeps one state per number.
//!
//! The invariant a later editor will break: a device keeps its number from its first report to its
//! removal, and no two devices of a stream share one.

use std::collections::HashMap;

use waft::ui::ReportedChange;

/// The numbers the devices of one stream go by now, by the stream's own numbers for them.
#[derive(Default)]
pub(crate) struct ForwardedDevices(HashMap<i64, i64>);

impl ForwardedDevices {
    /// The number the stream's `device` goes by in a report of `change`: a new one for a device
    /// the stream has not reported since it was last removed, let go with its removal. None for
    /// the removal of a device never reported, which has nothing to remove, or when the system
    /// gives no random number.
    pub(crate) fn number(&mut self, device: i64, change: &ReportedChange) -> Option<i64> {
        let removed = matches!(change, ReportedChange::Remove);
        let number = match self.0.get(&device) {
            Some(number) => *number,
            None if removed => return None,
            None => {
                let drawn = self.draw()?;
                self.0.insert(device, drawn);
                drawn
            }
        };
        if removed {
            self.0.remove(&device);
        }
        Some(number)
    }

    /// A random number no device of the stream goes by now.
    fn draw(&self) -> Option<i64> {
        loop {
            let drawn = getrandom::u64()
                .inspect_err(|error| tracing::warn!(%error, "no random number for a device"))
                .ok()?;
            // Every bit of the random number, as Waft's `i64`.
            let drawn = drawn as i64;
            if !self.0.values().any(|taken| *taken == drawn) {
                return Some(drawn);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOVER: ReportedChange = ReportedChange::Hover { buttons: 0 };

    #[test]
    fn a_device_keeps_its_number_until_it_is_removed() {
        let mut devices = ForwardedDevices::default();
        let first = devices.number(0, &ReportedChange::Add).unwrap();
        assert_eq!(devices.number(0, &HOVER), Some(first));
        assert_eq!(devices.number(0, &ReportedChange::Remove), Some(first));
        let again = devices.number(0, &ReportedChange::Add).unwrap();
        assert_ne!(again, first, "a device added again is a new one");
    }

    #[test]
    fn two_devices_of_a_stream_go_by_numbers_of_their_own() {
        let mut devices = ForwardedDevices::default();
        let mouse = devices.number(0, &HOVER).unwrap();
        let touch = devices
            .number(1, &ReportedChange::Down { buttons: 1 })
            .unwrap();
        assert_ne!(mouse, touch);
        assert_ne!(
            mouse, 0,
            "not the stream's own number, which WaOS's host may give"
        );
    }

    #[test]
    fn a_device_never_reported_has_nothing_to_remove() {
        let mut devices = ForwardedDevices::default();
        assert_eq!(devices.number(3, &ReportedChange::Remove), None);
    }
}
