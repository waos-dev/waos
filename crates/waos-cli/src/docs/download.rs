//! The docs fetched over HTTP: a version's archive from where its release's files are, and
//! waos.app's latest docs where there is none.

use anyhow::{Context, Result};

use super::page::SITE;

/// Where each release's files are, by its tag, `v<version>`, in the order they are asked: the copy
/// on download.waos.app, which `mirror-release.yml` makes and which is quick everywhere, then
/// GitHub's own, which can be slow in places.
const RELEASES: [&str; 2] = [
    "https://download.waos.app",
    "https://github.com/waos-dev/waos/releases/download",
];

/// Where `version`'s archive is, in the order to ask.
pub fn archive_urls(version: &str) -> Vec<String> {
    RELEASES
        .iter()
        .map(|releases| format!("{releases}/v{version}/waos-docs-{version}.tar.gz"))
        .collect()
}

/// `version`'s archive, as its release carries it: from the first place that has it.
pub async fn archive(version: &str) -> Result<Vec<u8>> {
    let mut failures = Vec::new();
    for url in archive_urls(version) {
        match fetched(&url).await {
            Ok(bytes) => return Ok(bytes),
            Err(failure) => failures.push(format!("{failure:#}")),
        }
    }
    anyhow::bail!("{}", failures.join("; "))
}

/// waos.app's docs, every page, and its playbook.
pub async fn latest() -> Result<(String, String)> {
    let pages = fetched(&format!("{SITE}/llms-full.txt")).await?;
    let playbook = fetched(&format!("{SITE}/agents.md")).await?;
    Ok((text(pages), text(playbook)))
}

async fn fetched(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("waos/", env!("CARGO_PKG_VERSION")))
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
