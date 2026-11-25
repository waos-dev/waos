//! What the store's tests install and read from: signed packages written to a folder, their
//! listings, and a local HTTP server.

#![allow(dead_code, reason = "each test crate uses its own part of the support")]

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use waos_package::{ListedProgram, Manifest, SigningKey};

/// The component of `id` at `version`: bytes that say which they are.
pub fn component_of(id: &str, version: &str) -> Vec<u8> {
    format!("\0asm {id} {version}").into_bytes()
}

/// A package of `id` at `version`, signed with `key`, written in `folder` as
/// `<id>-<version>.wapk`.
pub fn write_package(folder: &Path, id: &str, version: &str, key: &SigningKey) -> PathBuf {
    let manifest =
        Manifest::new(id.parse().unwrap(), "Triangle", version.parse().unwrap()).unwrap();
    let component = folder.join(format!("{id}-{version}.wasm"));
    fs::write(&component, component_of(id, version)).unwrap();
    let icon = folder.join("icon.png");
    fs::write(&icon, png_header(256)).unwrap();
    let path = folder.join(format!("{id}-{version}.wapk"));
    waos_package::write(&path, &manifest, &component, &icon, key).unwrap();
    path
}

/// The listing of the package at `path`, of `id` at `version`, by its file name.
pub fn listing(path: &Path, id: &str, version: &str) -> ListedProgram {
    ListedProgram::new(
        id.parse().unwrap(),
        "Triangle",
        version.parse().unwrap(),
        path.file_name().unwrap().to_string_lossy(),
        Sha256::digest(fs::read(path).unwrap()).into(),
    )
}

/// The start of a PNG of `side` by `side`, all an icon's check reads.
fn png_header(side: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&side.to_be_bytes());
    png.extend_from_slice(&side.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    png
}

/// An HTTP server on this machine, answering `GET`s of the files it was given, and 404 to
/// anything else, from a task of the runtime it started on.
pub struct Server {
    address: SocketAddr,
}

impl Server {
    pub async fn serve(files: Vec<(String, Vec<u8>)>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let files = Arc::new(files.into_iter().collect::<HashMap<_, _>>());
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(answer(stream, files.clone()));
            }
        });
        Server { address }
    }

    /// The URL of the file `path` on this server.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.address)
    }
}

/// Answers the one request `stream` carries, then closes it.
async fn answer(mut stream: TcpStream, files: Arc<HashMap<String, Vec<u8>>>) {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|end| end == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await.unwrap();
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let (status, body) = match files.get(path.trim_start_matches('/')) {
        Some(body) => ("200 OK", body.clone()),
        None => ("404 Not Found", b"not found".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    let _ = stream.shutdown().await;
}
