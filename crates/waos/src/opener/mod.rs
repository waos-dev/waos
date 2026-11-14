//! `wasdk:platform/opener`: opening what a program names in the app the system picks, a link, a
//! file, and a file shown in the system's file manager.
//!
//! The shell opens ([`ShellOpener`]); the engine keeps the rules: which links a program may open
//! ([`LinkSchemes`]: `http`, `https` and `mailto` freely, any other scheme as its user answered,
//! asked once per scheme through the shell's prompts and kept among its grants), and where on the
//! machine a file the program names is, through its mounts. The interface is served to a program
//! by `host`.
//!
//! The invariant: nothing the rules refuse reaches the shell, neither a link of a scheme the user
//! did not allow nor a path outside the program's files.

pub(crate) mod host;

use std::future::{Future, ready};
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use waos_capabilities::{Answer, ProgramGrants};

use crate::asking::Asking;
use crate::running::RunningProgram;
use crate::shell::{Prompts, ShellOpener};

/// The schemes whose links open without asking: the web's and mail's, which every system opens in
/// an app its user chose for them.
const FREE_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// `opener.opener`: the shell's opener, as the program reaches it.
pub struct Opener(Arc<dyn ShellOpener>);

/// Which schemes of links a program may open: the free ones, and any other its user allowed,
/// asked once per scheme.
pub(crate) struct LinkSchemes {
    grants: Arc<dyn ProgramGrants>,
    prompts: Arc<dyn Prompts>,
    program: RunningProgram,
    /// The schemes being asked about now, each once however often the program opens one.
    asking: Asking<String>,
}

impl LinkSchemes {
    /// `program`'s link schemes, as `grants` keep the user's answers and `prompts` ask the user.
    pub(crate) fn new(
        grants: Arc<dyn ProgramGrants>,
        prompts: Arc<dyn Prompts>,
        program: RunningProgram,
    ) -> LinkSchemes {
        LinkSchemes {
            grants,
            prompts,
            program,
            asking: Asking::default(),
        }
    }

    /// Resolves to whether the program may open links of `scheme`, in lower case: at once for a
    /// free scheme or one its user answered; for any other once the user answers, the answer
    /// kept. Nobody answering refuses it, and leaves it to be asked again.
    pub(crate) fn allow(&self, scheme: &str) -> BoxFuture<'static, bool> {
        if FREE_SCHEMES.contains(&scheme) {
            return ready(true).boxed();
        }
        match self.grants.link_scheme_answer(scheme) {
            Some(answer) => ready(answer == Answer::Granted).boxed(),
            None => self.ask(scheme).boxed(),
        }
    }

    /// Asks the user about `scheme`, unless it is being asked already, and resolves to whether
    /// the answer allows it, kept through the grants once it comes.
    fn ask(&self, scheme: &str) -> impl Future<Output = bool> + Send + 'static {
        let (grants, kept) = (Arc::clone(&self.grants), scheme.to_owned());
        let asked = self.asking.ask(
            scheme.to_owned(),
            || self.prompts.ask_to_open_links(&self.program, scheme),
            move |answer| grants.set_link_scheme_answer(&kept, answer),
        );
        asked.map(|answer| answer == Some(Answer::Granted))
    }
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
}
