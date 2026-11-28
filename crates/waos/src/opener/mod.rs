//! `wasdk:platform/opener`: opening what a program names in the app the system picks, a link, a
//! file, and a file shown in the system's file manager.
//!
//! The shell opens ([`ShellOpener`]); the engine keeps the rules: which links a program may open
//! ([`LinkSchemes`]: `http`, `https` and `mailto` freely, any other scheme as its user answered,
//! which its grants ask once per scheme and keep, each link decided and told through the program's
//! access, `access`), and where on the machine a file the program
//! names is, through its mounts. The interface is served to a program by `host`.
//!
//! The invariant: nothing the rules refuse reaches the shell, neither a link of a scheme the user
//! did not allow nor a path outside the program's files.

pub(crate) mod host;

use std::future::{Future, ready};
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use waos_capabilities::{Answer, ProgramGrants};

use crate::access::{Asked, ProgramAccess, Prompt, Refusal};
use crate::shell::ShellOpener;

/// The schemes whose links open without asking: the web's and mail's, which every system opens in
/// an app its user chose for them.
const FREE_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// `opener.opener`: the shell's opener, as the program reaches it.
pub struct Opener(Arc<dyn ShellOpener>);

/// Which schemes of links a program may open: the free ones, and any other its user allowed,
/// asked once per scheme.
pub(crate) struct LinkSchemes {
    /// Where each link is decided, by the answers its grants keep, which ask the user.
    access: ProgramAccess,
}

impl LinkSchemes {
    /// A program's link schemes, as the grants `access` reads ask the user and keep the answers.
    pub(crate) fn new(access: ProgramAccess) -> LinkSchemes {
        LinkSchemes { access }
    }

    /// Resolves to whether the program may open `link`, of `scheme`, in lower case: at once for a
    /// free scheme or one its user answered; for any other once the user answers, the answer
    /// kept. Nobody answering refuses it, and leaves it to be asked again.
    pub(crate) fn allow(&self, link: &str, scheme: &str) -> BoxFuture<'static, bool> {
        let answered = FREE_SCHEMES.contains(&scheme)
            || self.access.grants().link_scheme_answer(scheme).is_some();
        let asked: BoxFuture<'static, Option<Answer>> = if answered {
            ready(None).boxed()
        } else {
            self.ask(scheme).boxed()
        };
        let (access, link, scheme) = (self.access.clone(), link.to_owned(), scheme.to_owned());
        async move { decide_link(&access, &link, &scheme, asked.await) }.boxed()
    }

    /// Asks the user about `scheme` through the grants, told as asked and answered: they ask it
    /// once however often the program opens its links, and keep the answer.
    fn ask(&self, scheme: &str) -> impl Future<Output = Option<Answer>> + Send + 'static {
        let asking = self.access.grants().ask_to_open_links(scheme);
        self.access
            .put_question(Prompt::LinkScheme(scheme.to_owned()), asking)
    }
}

/// Whether the program `access` is of may open `link`, of `scheme`: a free scheme freely, any
/// other as its user answered, the answer kept or else `asked`, the one just given.
fn decide_link(access: &ProgramAccess, link: &str, scheme: &str, asked: Option<Answer>) -> bool {
    let free_or_allowed = |grants: &dyn ProgramGrants| {
        let allowed = FREE_SCHEMES.contains(&scheme)
            || grants.link_scheme_answer(scheme).or(asked) == Some(Answer::Granted);
        if allowed {
            Ok(())
        } else {
            Err(Refusal::by("link-scheme"))
        }
    };
    access
        .decide(Asked::OpenLink { link }, free_or_allowed)
        .is_ok()
}

/// The scheme of `link`, in lower case, as RFC 3986 spells one before the first `:`: a letter,
/// then letters, digits, `+`, `-` and `.`. None for text that is no link.
fn scheme_of(link: &str) -> Option<String> {
    let (scheme, _rest) = link.split_once(':')?;
    let mut characters = scheme.chars();
    let starts_with_letter = characters.next()?.is_ascii_alphabetic();
    let spelled = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'));
    (starts_with_letter && spelled).then(|| scheme.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_links_scheme_is_what_comes_before_its_first_colon_in_lower_case() {
        assert_eq!(scheme_of("https://example.com/").as_deref(), Some("https"));
        assert_eq!(
            scheme_of("MailTo:me@example.com").as_deref(),
            Some("mailto")
        );
        assert_eq!(
            scheme_of("x-web+search.v2:query").as_deref(),
            Some("x-web+search.v2")
        );
        for text in ["no scheme", "", ":empty", "2fa:code", "a b:c", "/a/path"] {
            assert_eq!(scheme_of(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_link_of_a_scheme_nobody_allowed_is_told_refused_by_its_row() {
        let answers = Arc::new(crate::access::Answers::default());
        let access = answers.access();
        assert!(decide_link(&access, "https://example.com/", "https", None));
        assert!(!decide_link(&access, "zoommtg://join", "zoommtg", None));
        assert!(decide_link(
            &access,
            "zoommtg://join",
            "zoommtg",
            Some(Answer::Granted)
        ));
        answers.set_link_scheme_answer("zoommtg", Answer::Denied);
        assert!(!decide_link(&access, "zoommtg://join", "zoommtg", None));
        assert_eq!(access.refusals(), ["link-scheme", "link-scheme"]);
    }
}
