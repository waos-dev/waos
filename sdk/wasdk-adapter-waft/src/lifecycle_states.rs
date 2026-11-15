//! The program's lifecycle state as the framework hears it: the state it starts in, which the
//! platform answers when the framework's binding is made, and each change after, which the loop
//! pushes. Both sides speak Flutter's `AppLifecycleState`, the platform's without `detached`, so
//! this is a rename; the framework fills in the states between a change's two ends.

use waft_embedder::AppLifecycleState;
use wasdk::lifecycle::LifecycleState;

/// The platform's state of the program, as the framework's.
pub(crate) fn framework_lifecycle_state(state: LifecycleState) -> AppLifecycleState {
    match state {
        LifecycleState::Resumed => AppLifecycleState::Resumed,
        LifecycleState::Inactive => AppLifecycleState::Inactive,
        LifecycleState::Hidden => AppLifecycleState::Hidden,
        LifecycleState::Paused => AppLifecycleState::Paused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_state_keeps_its_name() {
        let states = [
            LifecycleState::Resumed,
            LifecycleState::Inactive,
            LifecycleState::Hidden,
            LifecycleState::Paused,
        ]
        .map(framework_lifecycle_state);
        assert_eq!(
            states,
            [
                AppLifecycleState::Resumed,
                AppLifecycleState::Inactive,
                AppLifecycleState::Hidden,
                AppLifecycleState::Paused,
            ]
        );
    }
}
