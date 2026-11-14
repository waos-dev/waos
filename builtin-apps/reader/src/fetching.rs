//! Fetching a feed: one request for its document, sent with what its server said of it last time,
//! and the document read into what the library keeps. For an address the user just gave, a page
//! that is no feed is asked which feed it names, and that one is fetched instead.

use url::Url;

use crate::feed::FetchedFeed;
use crate::feed_discovery::feed_named_by;
use crate::feed_document;
use crate::http::{self, Answer, Document, Validators};

/// What a feed's request says it accepts: the feed formats first, any document last, since many
/// servers send a feed under a general type.
const ACCEPTED: &str = "application/atom+xml, application/rss+xml, application/feed+json;q=0.9, application/xml;q=0.8, text/xml;q=0.8, */*;q=0.5";

/// The longest document read as a feed, in bytes.
const LONGEST_FEED: usize = 8 * 1024 * 1024;

/// What a feed's server answered.
#[derive(Debug)]
pub(crate) enum Fetched {
    /// Its document is as it was at the last refresh.
    Unchanged,
    Feed(Box<FetchedDocument>),
}

/// A feed's document, read.
#[derive(Debug)]
pub(crate) struct FetchedDocument {
    pub(crate) feed: FetchedFeed,
    pub(crate) validators: Validators,
    /// Where its server says the document now is for good.
    pub(crate) moved_to: Option<String>,
}

/// Why a feed was not fetched, as the app tells its user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NotFetched(pub(crate) String);

/// Fetches the feed at `address`, last described by `validators`.
pub(crate) async fn fetch_feed(
    address: &str,
    validators: &Validators,
) -> Result<Fetched, NotFetched> {
    let address = Url::parse(address).map_err(|_| not_fetched(&http::HttpError::BadAddress))?;
    match get(&address, validators).await? {
        Answer::Unchanged => Ok(Fetched::Unchanged),
        Answer::Document(document) => {
            let read = read(document, &address).map_err(|_| no_feed())?;
            Ok(Fetched::Feed(Box::new(read)))
        }
    }
}

/// Fetches the feed at `address`, or the feed the page at `address` names; answers the feed's own
/// address with it.
pub(crate) async fn find_feed(address: Url) -> Result<(Url, FetchedDocument), NotFetched> {
    let document = fresh(&address).await?;
    let page = document.body.clone();
    if let Ok(read) = read(document, &address) {
        return Ok((address, read));
    }
    let named = feed_named_by(&page, &address).ok_or_else(no_feed)?;
    let document = fresh(&named).await?;
    let read = read(document, &named).map_err(|_| no_feed())?;
    Ok((named, read))
}

async fn get(address: &Url, validators: &Validators) -> Result<Answer, NotFetched> {
    let answer = http::get(address, ACCEPTED, validators, LONGEST_FEED).await;
    answer.map_err(|error| not_fetched(&error))
}

/// The document at `address`, asked for with no validators, so never answered "unchanged".
async fn fresh(address: &Url) -> Result<Document, NotFetched> {
    match get(address, &Validators::default()).await? {
        Answer::Document(document) => Ok(document),
        Answer::Unchanged => Err(no_feed()),
    }
}

fn read(document: Document, address: &Url) -> Result<FetchedDocument, feed_document::NotAFeed> {
    // Read against where the document was found, not where it was asked for.
    let found_at = document.moved_to.as_ref().unwrap_or(address);
    let feed = feed_document::read(&document.body, found_at)?;
    Ok(FetchedDocument {
        feed,
        validators: document.validators,
        moved_to: document.moved_to.map(String::from),
    })
}

fn not_fetched(error: &http::HttpError) -> NotFetched {
    NotFetched(error.said())
}

fn no_feed() -> NotFetched {
    NotFetched("There is no feed at this address.".to_owned())
}
