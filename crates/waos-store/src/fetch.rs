//! Reading from a package source over HTTP: an index and a picture, each whole within a cap, and
//! a package, streamed as it arrives and checked against the SHA-256 its listing gives.
//!
//! The invariant a later editor will break: a download's bytes end in an error unless their
//! SHA-256 is the one listed, so whoever takes them in installs nothing else.

use std::io;
use std::time::Duration;

use futures::StreamExt as _;
use futures::stream::{self, BoxStream};
use reqwest::{Client, Response};
use sha2::{Digest, Sha256};
use url::Url;
use waos_package::Index;

use crate::{SourceError, StoreError};

/// The most of an index the store reads: tens of thousands of listings.
const INDEX_LIMIT: u64 = 4 * 1024 * 1024;

/// The most of a picture the store reads: an icon at its largest, several times over.
const PICTURE_LIMIT: u64 = 8 * 1024 * 1024;

/// How long a server may take to accept a connection, and then to send each next part of an
/// answer, before a read fails rather than waits on.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Which proxy the store reads sources through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceProxy {
    /// The system's proxy settings, as reqwest reads them: how an app reads.
    System,
    /// None: each source is read straight from its server, whatever proxy the machine has set,
    /// so a test's sources are read from what it serves on this computer.
    None,
}

/// The client every source is read with, through `proxy`.
pub(crate) fn http_client(proxy: SourceProxy) -> Result<Client, StoreError> {
    let client = match proxy {
        SourceProxy::System => Client::builder(),
        SourceProxy::None => Client::builder().no_proxy(),
    };
    client
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(StoreError::Http)
}

/// The index at `url`.
pub(crate) async fn read_index(http: &Client, url: &Url) -> Result<Index, SourceError> {
    let mut response = get(http, url).await?;
    let mut json = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(SourceError::unreachable_at(url))?
    {
        if (json.len() + chunk.len()) as u64 > INDEX_LIMIT {
            return Err(SourceError::IndexTooLarge {
                url: url.clone(),
                limit: INDEX_LIMIT,
            });
        }
        json.extend_from_slice(&chunk);
    }
    Index::from_json(&json).map_err(|error| SourceError::NotAnIndex {
        url: url.clone(),
        error,
    })
}

/// The picture at `url`, whole.
pub(crate) async fn read_picture(http: &Client, url: &Url) -> Result<Vec<u8>, SourceError> {
    let mut response = get(http, url).await?;
    let mut picture = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(SourceError::unreachable_at(url))?
    {
        if (picture.len() + chunk.len()) as u64 > PICTURE_LIMIT {
            return Err(SourceError::PictureTooLarge {
                url: url.clone(),
                limit: PICTURE_LIMIT,
            });
        }
        picture.extend_from_slice(&chunk);
    }
    Ok(picture)
}

/// A package downloading.
pub struct Download {
    /// Its bytes as they arrive, ending in an error where the download fails or their SHA-256 is
    /// not the one listed.
    pub bytes: BoxStream<'static, io::Result<Vec<u8>>>,
    /// Their length, where the server said it.
    pub size: Option<u64>,
}

/// What a download keeps between two parts.
struct Downloading {
    response: Response,
    sha256: Sha256,
    url: Url,
    listed: [u8; 32],
}

/// Starts downloading the package at `url`, whose SHA-256 is `listed`.
pub(crate) async fn download(
    http: &Client,
    url: &Url,
    listed: &[u8; 32],
) -> Result<Download, SourceError> {
    let response = get(http, url).await?;
    let size = response.content_length();
    let downloading = Downloading {
        response,
        sha256: Sha256::new(),
        url: url.clone(),
        listed: *listed,
    };
    let bytes = stream::unfold(Some(downloading), |downloading| async move {
        let mut downloading = downloading?;
        match downloading.response.chunk().await {
            Ok(Some(chunk)) => {
                downloading.sha256.update(&chunk);
                Some((Ok(chunk.to_vec()), Some(downloading)))
            }
            Ok(None) => {
                let downloaded: [u8; 32] = downloading.sha256.finalize().into();
                (downloaded != downloading.listed).then(|| {
                    let mismatch = SourceError::Sha256Mismatch {
                        listed: downloading.listed,
                        downloaded,
                    };
                    (Err(io::Error::other(mismatch)), None)
                })
            }
            Err(error) => {
                let unreachable = SourceError::Unreachable {
                    url: downloading.url,
                    error,
                };
                Some((Err(io::Error::other(unreachable)), None))
            }
        }
    })
    .boxed();
    Ok(Download { bytes, size })
}

/// The answer to a `GET` of `url`, refused unless its status is success.
async fn get(http: &Client, url: &Url) -> Result<Response, SourceError> {
    let response = http
        .get(url.clone())
        .send()
        .await
        .map_err(SourceError::unreachable_at(url))?;
    let status = response.status();
    if !status.is_success() {
        return Err(SourceError::Status {
            url: url.clone(),
            status,
        });
    }
    Ok(response)
}
