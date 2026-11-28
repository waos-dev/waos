//! What the activity record keeps: an installed app's last seven days, newest first, while its
//! user keeps a record of it; and nothing of it once it is removed or its record is turned off.
//! Each over the real record and Permissions in memory, with the package manager's record played
//! by the test.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use tokio::sync::{mpsc, watch};
use waos_capabilities::{AuditDay, AuditLog, DAYS_KEPT, Setting, Uses};
use waos_package::ProgramId;
use waos_services::activity::{ActivityInMemory, ActivityRecord, ActivityStore};
use waos_services::packages::InstalledApps;
use waos_services::permissions::{NoPrompts, Permissions, SettingsInMemory};

/// The package manager's record, as a test installs and removes apps.
#[derive(Default)]
struct Installed {
    apps: Mutex<BTreeSet<ProgramId>>,
    removing: watch::Sender<BTreeSet<ProgramId>>,
    listeners: Mutex<Vec<mpsc::UnboundedSender<ProgramId>>>,
}

impl Installed {
    fn install(&self, app: &ProgramId) {
        self.apps.lock().unwrap().insert(app.clone());
    }

    /// `app` is out of the record, and its removal told.
    fn remove(&self, app: &ProgramId) {
        self.apps.lock().unwrap().remove(app);
        for listener in self.listeners.lock().unwrap().iter() {
            let _ = listener.send(app.clone());
        }
    }
}

impl InstalledApps for Installed {
    fn name(&self, id: &ProgramId) -> Option<String> {
        self.apps
            .lock()
            .unwrap()
            .contains(id)
            .then(|| id.to_string())
    }

    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.removing.subscribe()
    }

    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        let (sender, removals) = mpsc::unbounded_channel();
        self.listeners.lock().unwrap().push(sender);
        removals
    }
}

/// The record over `store`, with Permissions in memory, the apps `installed` holds.
fn record_over(
    store: &Arc<ActivityInMemory>,
    installed: &Arc<Installed>,
) -> (ActivityRecord, Permissions) {
    let permissions = Permissions::open(
        Arc::new(SettingsInMemory::default()),
        Arc::new(NoPrompts),
        Arc::clone(installed) as Arc<dyn InstalledApps>,
    );
    let record = ActivityRecord::open(
        Arc::clone(store) as Arc<dyn ActivityStore>,
        permissions.clone(),
        Arc::clone(installed) as Arc<dyn InstalledApps>,
    )
    .unwrap();
    (record, permissions)
}

fn notes() -> ProgramId {
    "dev.waos.notes".parse().unwrap()
}

fn camera() -> ProgramId {
    "dev.waos.camera".parse().unwrap()
}

/// The day `days_since_epoch` days after 1970, on which the app posted a notification.
fn day(days_since_epoch: u64) -> AuditDay {
    let mut day = AuditDay::starting(UNIX_EPOCH + Duration::from_secs(days_since_epoch * 86_400));
    day.notifications = Uses {
        allowed: 1,
        ..Uses::default()
    };
    day
}

#[test]
fn an_apps_last_seven_days_are_kept_newest_first_and_go_with_its_removal() {
    let (store, installed) = (Arc::default(), Arc::new(Installed::default()));
    let (record, _permissions) = record_over(&store, &installed);
    installed.install(&notes());
    for days in 20_400..20_410 {
        record.keep("dev.waos.notes", &day(days));
    }
    let mut today = day(20_409);
    today.pastes = 3;
    record.keep("dev.waos.notes", &today);
    let kept = record.days("dev.waos.notes");
    assert_eq!(kept.len(), DAYS_KEPT);
    assert_eq!(
        kept[0], today,
        "the day kept last, in place of the one before"
    );
    assert!(kept.windows(2).all(|pair| pair[0].start > pair[1].start));
    assert_eq!(
        store.days(&notes()).unwrap().len(),
        DAYS_KEPT,
        "the older ones removed"
    );
    assert_eq!(record.apps(), [notes()]);

    installed.remove(&notes());
    assert!(
        record.days("dev.waos.notes").is_empty(),
        "nothing of it once removed"
    );
    assert!(record.apps().is_empty());
    record.keep("dev.waos.notes", &day(20_410));
    installed.install(&notes());
    assert!(
        record.days("dev.waos.notes").is_empty(),
        "nothing kept of a run that ended after its removal"
    );
}

#[test]
fn what_is_kept_of_an_app_not_installed_goes_as_the_record_opens() {
    let (store, installed) = (Arc::new(ActivityInMemory::default()), Arc::default());
    store.keep(&notes(), &day(20_400)).unwrap();
    let (record, _permissions) = record_over(&store, &installed);
    assert!(store.apps().unwrap().is_empty());
    record.keep("dev.waos.never-installed", &day(20_400));
    record.keep("../elsewhere", &day(20_400));
    assert!(
        store.apps().unwrap().is_empty(),
        "nothing kept of a name no app installed has"
    );
}

#[test]
fn an_app_whose_record_is_off_has_nothing_kept() {
    let (store, installed) = (Arc::default(), Arc::new(Installed::default()));
    let (record, permissions) = record_over(&store, &installed);
    installed.install(&notes());
    installed.install(&camera());
    record.keep("dev.waos.notes", &day(20_400));
    record.keep("dev.waos.camera", &day(20_400));
    permissions
        .change_app_settings(&notes(), |own| own.record = Setting::Own(false))
        .unwrap();
    assert!(
        record.days("dev.waos.notes").is_empty(),
        "none as it is turned off"
    );
    record.keep("dev.waos.notes", &day(20_401));
    assert_eq!(record.days("dev.waos.camera").len(), 1, "another's stays");

    let reopened = ActivityRecord::open(
        Arc::clone(&store) as Arc<dyn ActivityStore>,
        permissions.clone(),
        Arc::clone(&installed) as Arc<dyn InstalledApps>,
    )
    .unwrap();
    assert!(
        store.days(&notes()).unwrap().is_empty(),
        "what was kept of it removed"
    );

    permissions
        .change_app_settings(&notes(), |own| own.record = Setting::Own(true))
        .unwrap();
    permissions
        .change_global_settings(|global| global.record = false)
        .unwrap();
    assert!(
        reopened.days("dev.waos.camera").is_empty(),
        "off for every app that follows"
    );
    reopened.keep("dev.waos.notes", &day(20_402));
    assert_eq!(
        reopened.days("dev.waos.notes"),
        [day(20_402)],
        "on by its own setting"
    );
}
