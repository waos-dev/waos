use std::path::Path;

use waos::{ProgramEnd, ProgramEnvironment};

use super::*;
use crate::logic::test_engine::{self, WITHIN};

#[tokio::test]
async fn where_there_can_be_no_apps_a_window_asked_for_fails_and_none_is_shown() {
    let placement = NoWindowProcesses::new("the window library is not there".to_owned());
    let engine = test_engine::engine(Arc::new(placement));
    let survey = engine
        .load(
            Path::new(waos_test_programs::SURVEY),
            &waos_test_programs::code_of(waos_test_programs::SURVEY),
        )
        .expect("survey loads");
    let program = engine
        .run(&survey, ProgramEnvironment::default())
        .expect("survey runs");
    let end = tokio::time::timeout(WITHIN, program.ended())
        .await
        .expect("survey ends once its window is refused");
    assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
}
