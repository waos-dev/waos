use std::path::Path;
use std::time::Duration;

use waos::{ProgramEnd, ProgramEnvironment};
use waos_test_programs::CODE_CACHE;

use super::*;
use crate::logic::test_engine;

/// How long a test waits for a program to do something.
const WITHIN: Duration = Duration::from_secs(60);

#[tokio::test]
async fn where_there_can_be_no_apps_a_window_asked_for_fails_and_none_is_shown() {
    let placement = NoProgramApps::new("the window library is not there".to_owned());
    let engine = test_engine::engine(Arc::new(placement), Path::new(CODE_CACHE));
    let survey = engine
        .load(Path::new(waos_test_programs::SURVEY))
        .expect("survey loads");
    let (program, _given) = engine
        .run(&survey, ProgramEnvironment::default(), Vec::new())
        .expect("survey runs");
    let end = tokio::time::timeout(WITHIN, program.ended())
        .await
        .expect("survey ends once its window is refused");
    assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
}
