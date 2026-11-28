//! Asking the user: a question an app waits for is asked once however many wait, its answer kept;
//! a change of the settings answers a question waiting and puts its prompt away; a question nobody
//! waits for any longer is put away; and what is asked now is one list, a settings page opened
//! for the user among it.

use std::sync::Arc;

use super::support::{Named, TestPrompts, triangle};
use futures::FutureExt;
use waos_services::permissions::{
    Answer, AppQuestion, AskedQuestion, InstalledApps, Permissions, Prompts, Question, Role,
    Setting, SettingsInMemory, Switch,
};

/// Permissions in memory, asking the user `prompts`, the triangle installed as Triangle.
fn asking(prompts: &Arc<TestPrompts>) -> Permissions {
    let prompts = Arc::clone(prompts) as Arc<dyn Prompts>;
    let installed: Arc<dyn InstalledApps> =
        Arc::new(Named(&[("dev.example.triangle", "Triangle")]));
    Permissions::open(Arc::new(SettingsInMemory::default()), prompts, installed)
}

fn asked(question: AppQuestion) -> AskedQuestion {
    AskedQuestion {
        app: triangle(),
        question,
    }
}

#[test]
fn a_question_asked_while_it_is_asked_is_asked_once_and_its_answer_kept() {
    let prompts = Arc::new(TestPrompts::default());
    let permissions = asking(&prompts);
    let (first, second) = (permissions.of(&triangle()), permissions.of(&triangle()));
    let (mut one, mut other) = (first.ask(Question::Camera), second.ask(Question::Camera));
    assert_eq!(one.as_mut().now_or_never(), None);
    assert_eq!(other.as_mut().now_or_never(), None);
    assert_eq!(
        prompts.asked(),
        [("Triangle".to_owned(), "Camera".to_owned())],
        "asked once, by the name the user knows"
    );
    assert_eq!(
        *permissions.asking().borrow(),
        [asked(AppQuestion::Permission(Question::Camera))]
    );

    prompts.answer(Some(Answer::Granted));
    assert_eq!(one.now_or_never(), Some(Some(Answer::Granted)));
    assert_eq!(other.now_or_never(), Some(Some(Answer::Granted)));
    assert!(permissions.asking().borrow().is_empty(), "no longer asked");
    let own = permissions.app_settings(&triangle()).unwrap();
    assert_eq!(
        *own.answers.get(Question::Camera),
        Setting::Own(Answer::Granted)
    );
    let again = first.ask(Question::Camera).now_or_never();
    assert_eq!(again, Some(Some(Answer::Granted)), "read, not asked");
    assert_eq!(prompts.asked().len(), 1);
}

#[test]
fn a_change_of_the_settings_answers_a_question_waiting_and_puts_its_prompt_away() {
    let prompts = Arc::new(TestPrompts::default());
    let permissions = asking(&prompts);
    let mut waiting = permissions.of(&triangle()).ask(Question::Microphone);
    assert_eq!(waiting.as_mut().now_or_never(), None);

    permissions
        .change_global_settings(|global| global.clipboard_read = false)
        .unwrap();
    assert_eq!(waiting.as_mut().now_or_never(), None, "another change");
    assert!(!prompts.put_away(0));

    permissions
        .change_app_settings(&triangle(), |own| {
            own.answers
                .set(Question::Microphone, Setting::Own(Answer::Simulated));
        })
        .unwrap();
    assert_eq!(waiting.now_or_never(), Some(Some(Answer::Simulated)));
    assert!(prompts.put_away(0), "the prompt is put away");
    assert!(permissions.asking().borrow().is_empty());
}

#[test]
fn a_question_nobody_waits_for_is_put_away_and_asked_again_at_the_next_wait() {
    let prompts = Arc::new(TestPrompts::default());
    let permissions = asking(&prompts);
    let grants = permissions.of(&triangle());
    let mut waiting = grants.ask_for_role(Role::Inspector);
    assert_eq!(waiting.as_mut().now_or_never(), None);
    drop(waiting);
    assert!(prompts.put_away(0));
    assert!(permissions.asking().borrow().is_empty());
    assert_eq!(grants.role(Role::Inspector), None, "nothing kept");

    let mut again = grants.ask_for_role(Role::Inspector);
    assert_eq!(again.as_mut().now_or_never(), None);
    assert_eq!(prompts.asked().len(), 2, "asked again");
    prompts.answer(None);
    assert_eq!(again.now_or_never(), Some(None), "nobody answered");
    assert_eq!(grants.role(Role::Inspector), None, "to be asked again");
}

#[test]
fn a_link_scheme_is_asked_once_and_kept_for_its_scheme_alone() {
    let prompts = Arc::new(TestPrompts::default());
    let permissions = asking(&prompts);
    let grants = permissions.of(&triangle());
    let mut zoom = grants.ask_to_open_links("zoommtg");
    assert_eq!(zoom.as_mut().now_or_never(), None, "asked once awaited");
    assert_eq!(
        *permissions.asking().borrow(),
        [asked(AppQuestion::LinkScheme("zoommtg".to_owned()))]
    );
    prompts.answer(Some(Answer::Denied));
    assert_eq!(zoom.as_mut().now_or_never(), Some(Some(Answer::Denied)));
    assert_eq!(grants.link_scheme_answer("zoommtg"), Some(Answer::Denied));
    assert_eq!(grants.link_scheme_answer("tel"), None);
}

#[test]
fn a_switchs_page_is_listed_while_it_is_open() {
    let prompts = Arc::new(TestPrompts::default());
    let permissions = asking(&prompts);
    let mut open = permissions.of(&triangle()).open_settings(Switch::Network);
    assert_eq!(prompts.pages(), [("Triangle".to_owned(), Switch::Network)]);
    assert_eq!(
        *permissions.asking().borrow(),
        [asked(AppQuestion::Switch(Switch::Network))]
    );
    assert_eq!(open.as_mut().now_or_never(), None);
    prompts.leave_pages();
    assert_eq!(open.now_or_never(), Some(()));
    assert!(permissions.asking().borrow().is_empty());
}
