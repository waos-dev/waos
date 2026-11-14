//! The machine's facts kept current in the engine: each time Waft says the screens, light or
//! dark, or the languages may have changed, the engine is set the whole value Waft answers now,
//! and works out itself what changed, so reporting more often than needed tells programs
//! nothing new; the caption style is set at each change Waft tells of it, which programs hear; and
//! the time zone, which Waft does not report, is read again at each change the system announces,
//! where WaOS hears it (`os::watch_time_zone`, macOS so far).
//!
//! The screens change is one of Waft's metrics changes, as Flutter's `_updateDisplays` is, so
//! every metrics change sets the screens again.

use std::rc::Rc;
use std::sync::Arc;

use waft::ui::Locale as WaftLocale;
use waft::{App, WidgetsBinding, WidgetsBindingObserver};
use waos::WaOS;

use crate::logic::caption_style::caption_style;
use crate::logic::machine_facts::{brightness, locales, screens};
use crate::os::{self, TimeZoneChanges};

/// From now on, what Waft reports of the machine reaches `engine`.
pub(crate) fn keep_current(app: &mut App, engine: Arc<WaOS>) {
    let time_zone_changes = watch_time_zone(Arc::clone(&engine));
    let reports = Rc::new(MachineReports {
        engine,
        _time_zone_changes: time_zone_changes,
    });
    WidgetsBinding::instance(app).add_observer(app, reports);
}

/// From now on, each change of the user's time zone the system announces reaches `engine`.
fn watch_time_zone(engine: Arc<WaOS>) -> Option<TimeZoneChanges> {
    os::watch_time_zone(move || engine.set_time_zone(os::time_zone()))
}

struct MachineReports {
    engine: Arc<WaOS>,
    /// What hears the time zone change, for as long as the reports are kept.
    _time_zone_changes: Option<TimeZoneChanges>,
}

impl WidgetsBindingObserver for MachineReports {
    fn did_change_metrics(&self, app: &mut App) {
        self.engine.set_screens(screens(app.platform().as_ref()));
    }

    fn did_change_platform_brightness(&self, app: &mut App) {
        self.engine
            .set_platform_brightness(brightness(app.platform().as_ref()));
    }

    fn did_change_locales(&self, app: &mut App, _locales: Option<&[WaftLocale]>) {
        self.engine.set_locales(locales(app.platform().as_ref()));
    }

    fn did_change_caption_style(&self, app: &mut App) {
        if let Some(style) = caption_style(app.platform().as_ref()) {
            self.engine.set_caption_style(style);
        }
    }
}
