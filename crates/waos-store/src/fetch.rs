//! Reading from a package source over HTTP: an index, whole within a cap, and a package, streamed
//! to a temporary file and checked against the SHA-256 its listing gives before anything opens it.
//!
//! The invariant a later editor will break: a download is handed on only when its SHA-256 is the
//! one listed; one that is not is removed with its temporary file.

use std::io::Write;
use std::time::Duration;

use reqwest::{Client, Response};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use url::Url;
use waos_package::Index;

use crate::{SourceError, StoreError};

/// The most of an index the store reads: tens of thousands of listings.
const INDEX_LIMIT: u64 = 4 * 1024 * 1024;

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

/// Downloads the package at `url` into a temporary file, removed when dropped, and refuses it
/// unless its SHA-256 is `listed`.
pub(crate) async fn download(
    http: &Client,
    url: &Url,
    listed: &[u8; 32],
) -> Result<NamedTempFile, SourceError> {
    let mut response = get(http, url).await?;
    let mut file = NamedTempFile::new().map_err(|error| SourceError::Io {
        path: std::env::temp_dir(),
        error,
    })?;
    let mut sha256 = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(SourceError::unreachable_at(url))?
    {
        sha256.update(&chunk);
        // A part of an answer is written to a local file in far less time than it took to come.
        file.write_all(&chunk).map_err(|error| SourceError::Io {
            path: file.path().to_path_buf(),
            error,
        })?;
    }
    let downloaded: [u8; 32] = sha256.finalize().into();
    if downloaded != *listed {
        return Err(SourceError::Sha256Mismatch {
            listed: *listed,
            downloaded,
        });
    }
    Ok(file)
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
