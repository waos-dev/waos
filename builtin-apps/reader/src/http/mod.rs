//! Fetching a document over HTTP as a polite client does: one `GET`, named by a plain user agent,
//! sent with what the server said of the document last time so that it may answer "unchanged",
//! led through a few redirects, ended by a deadline and by a limit on its length.
//!
//! WaOS's HTTP follows no redirect and tells the program of each, so following is here
//! ([`get`]); one request and its answer is [`exchange`]'s.

mod exchange;

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use url::Url;

/// What the app calls itself to a server: a feed reader, and nothing of its user.
const USER_AGENT: &str = "WaOS-Reader/0.2 (feed reader)";

/// The most redirects followed for one document.
const MOST_REDIRECTS: usize = 5;

/// How long one document may take, redirects and all.
const WITHIN: Duration = Duration::from_secs(45);

/// What a server says of a document to tell later whether it changed: HTTP's `ETag` and
/// `Last-Modified`, sent back as `If-None-Match` and `If-Modified-Since`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Validators {
    pub(crate) etag: Option<String>,
    pub(crate) last_modified: Option<String>,
}

/// A document as a server gave it.
#[derive(Debug)]
pub(crate) struct Document {
    pub(crate) body: Vec<u8>,
    pub(crate) validators: Validators,
    /// Where the server says the document now is for good, where it led there by permanent
    /// redirects alone: the address to ask next time.
    pub(crate) moved_to: Option<Url>,
}

#[derive(Debug)]
pub(crate) enum Answer {
    Document(Document),
    /// The server says the document is as the validators sent describe it.
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HttpError {
    /// The address is none a request can be made to.
    BadAddress,
    /// The user's settings in WaOS keep the program from the network, or from this address.
    NotAllowed,
    /// No server was reached: no network, no such name, or nothing listening.
    Unreachable,
    TimedOut,
    /// The server's certificate was refused, or the secure connection failed.
    NotSecure,
    /// The server answered with a status that is no success.
    Status(u16),
    /// The document is longer than the limit asked for.
    TooLarge,
    /// The server led through more redirects than are followed, or to no address.
    BadRedirect,
    /// The request failed for a reason WaOS gives no name to; what it said went to the log.
    Failed,
}

impl HttpError {
    /// Why the document did not come, as the app tells its user.
    pub(crate) fn said(&self) -> String {
        match self {
            HttpError::BadAddress => "The address is not one Reader can fetch.".to_owned(),
            HttpError::NotAllowed => {
                "WaOS’s settings do not let Reader reach this address.".to_owned()
            }
            HttpError::Unreachable => "The server could not be reached.".to_owned(),
            HttpError::TimedOut => "The server took too long to answer.".to_owned(),
            HttpError::NotSecure => "A secure connection could not be made.".to_owned(),
            HttpError::Status(404 | 410) => "Nothing is at this address.".to_owned(),
            HttpError::Status(status) => format!("The server answered with an error ({status})."),
            HttpError::TooLarge => "What is at this address is too large.".to_owned(),
            HttpError::BadRedirect => "The server sent Reader in circles.".to_owned(),
            HttpError::Failed => "The request failed.".to_owned(),
        }
    }
}

/// Fetches the document at `address`, no longer than `limit` bytes, saying it accepts `accept`
/// and sending `validators`; follows the redirects the server answers with.
pub(crate) async fn get(
    address: &Url,
    accept: &str,
    validators: &Validators,
    limit: usize,
) -> Result<Answer, HttpError> {
    let deadline = Instant::now() + WITHIN;
    let headers = headers(accept, validators);
    let mut asked = address.clone();
    let mut moved_for_good = true;
    for _ in 0..=MOST_REDIRECTS {
        let reply = exchange::get(&asked, &headers, limit, deadline).await?;
        match reply.status {
            200..=299 => {
                let moved = moved_for_good && asked != *address;
                return Ok(Answer::Document(Document {
                    validators: validators_of(&reply),
                    body: reply.body,
                    moved_to: moved.then_some(asked),
                }));
            }
            304 => return Ok(Answer::Unchanged),
            status @ (301 | 302 | 303 | 307 | 308) => {
                moved_for_good &= matches!(status, 301 | 308);
                asked = redirected(&asked, reply.header("location"))?;
            }
            status => return Err(HttpError::Status(status)),
        }
    }
    Err(HttpError::BadRedirect)
}

/// The request's headers: who asks, what it accepts, and what the server said last time.
fn headers<'a>(accept: &'a str, validators: &'a Validators) -> Vec<(&'a str, &'a str)> {
    let mut headers = vec![("user-agent", USER_AGENT), ("accept", accept)];
    if let Some(etag) = &validators.etag {
        headers.push(("if-none-match", etag));
    }
    if let Some(last_modified) = &validators.last_modified {
        headers.push(("if-modified-since", last_modified));
    }
    headers
}

fn validators_of(reply: &exchange::Reply) -> Validators {
    Validators {
        etag: reply.header("etag").map(str::to_owned),
        last_modified: reply.header("last-modified").map(str::to_owned),
    }
}

/// Where a redirect from `asked` to `location` leads: a web address, read against `asked`.
fn redirected(asked: &Url, location: Option<&str>) -> Result<Url, HttpError> {
    let location = location.ok_or(HttpError::BadRedirect)?;
    let next = asked.join(location).map_err(|_| HttpError::BadRedirect)?;
    if matches!(next.scheme(), "http" | "https") {
        Ok(next)
    } else {
        Err(HttpError::BadRedirect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_is_read_against_the_address_asked() {
        let asked = Url::parse("https://nasa.gov/feeds/old/").unwrap();
        let to = |location| redirected(&asked, Some(location)).map(String::from);
        assert_eq!(
            to("https://www.nasa.gov/feed/").unwrap(),
            "https://www.nasa.gov/feed/"
        );
        assert_eq!(to("/feed/").unwrap(), "https://nasa.gov/feed/");
        assert_eq!(to("../new/").unwrap(), "https://nasa.gov/feeds/new/");
        assert_eq!(to("file:///etc/hosts"), Err(HttpError::BadRedirect));
        assert_eq!(redirected(&asked, None), Err(HttpError::BadRedirect));
    }

    #[test]
    fn the_validators_kept_are_sent_back() {
        let validators = Validators {
            etag: Some("\"abc\"".to_owned()),
            last_modified: None,
        };
        let sent = headers("application/xml", &validators);
        assert!(sent.contains(&("if-none-match", "\"abc\"")));
        assert!(sent.iter().all(|(name, _)| *name != "if-modified-since"));
        assert!(sent.contains(&("user-agent", USER_AGENT)));
    }
}
