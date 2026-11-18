//! The docs fetched over HTTP: a version's archive from its GitHub release, and waos.app's latest
//! docs where there is none.

use anyhow::{Context, Result};

use super::page::SITE;

/// Where each release's files are, by its tag, `v<version>`.
const RELEASES: &str = "https://github.com/waos-dev/waos/releases/download";

/// Where `version`'s archive is.
pub fn archive_url(version: &str) -> String {
    format!("{RELEASES}/v{version}/waos-docs-{version}.tar.gz")
}

/// `version`'s archive, as its release carries it.
pub async fn archive(version: &str) -> Result<Vec<u8>> {
    fetched(&archive_url(version)).await
}

/// waos.app's docs, every page, and its playbook.
pub async fn latest() -> Result<(String, String)> {
    let pages = fetched(&format!("{SITE}/llms-full.txt")).await?;
    let playbook = fetched(&format!("{SITE}/agents.md")).await?;
    Ok((text(pages), text(playbook)))
}

async fn fetched(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("cargo-waos/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("an HTTP client")?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("asking for {url}"))?;
    let response = response
        .error_for_status()
        .with_context(|| format!("asking for {url}"))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("reading {url}"))?;
    Ok(bytes.to_vec())
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}
