//! What a picker or a dialog answers where its user may have set the answer in the program's
//! settings ([`stored_or_shown`]): the answer set, without showing anything, or else what is shown,
//! raced against a change of the settings that sets one, which puts it away.
//!
//! The invariant: a change between reading no answer and waiting is never missed, as the next
//! change is asked for before each read.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;

use futures::future::{Either, FutureExt, select};
use waos_capabilities::ProgramGrants;

/// What a picker or a dialog came to: what was shown answered, or the answer set in the program's
/// settings.
pub(crate) enum Outcome<P, S> {
    Prompt(P),
    Setting(S),
}

/// Resolves once `grants` holds an answer `read` finds.
async fn settled<S>(
    grants: Arc<dyn ProgramGrants>,
    mut read: impl FnMut(&dyn ProgramGrants) -> Option<S>,
) -> S {
    loop {
        let changed = grants.changed();
        if let Some(answer) = read(&*grants) {
            return answer;
        }
        changed.await;
    }
}

/// Answers what `read` already finds in `grants` without calling `show`, or else calls `show` and
/// races it against `grants` changing to an answer `read` finds, dropping `show`'s future if a
/// stored answer wins first.
pub(crate) async fn stored_or_shown<P, S, F: Future<Output = P>>(
    grants: Arc<dyn ProgramGrants>,
    read: impl FnMut(&dyn ProgramGrants) -> Option<S>,
    show: impl FnOnce() -> F,
) -> Outcome<P, S> {
    let mut setting = pin!(settled(grants, read));
    if let Some(answer) = setting.as_mut().now_or_never() {
        return Outcome::Setting(answer);
    }
    let shown = pin!(show());
    match select(setting, shown).await {
        Either::Left((answer, _)) => Outcome::Setting(answer),
        Either::Right((answered, _)) => Outcome::Prompt(answered),
    }
}
