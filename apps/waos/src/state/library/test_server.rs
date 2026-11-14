//! An HTTP server on this machine for the library's tests: the sources they read and the
//! packages they download, never the internet.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use waos_package::{Index, ListedProgram};

/// An HTTP server on this machine, on a runtime of its own, answering `GET`s of the files
/// published on it and 404 to anything else; it stops with its runtime.
pub(crate) struct Server {
    address: SocketAddr,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    _runtime: tokio::runtime::Runtime,
}

impl Server {
    pub(crate) fn start() -> Server {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_io()
            .build()
            .unwrap();
        let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let address = listener.local_addr().unwrap();
        let files = Arc::new(Mutex::new(HashMap::new()));
        let served = Arc::clone(&files);
        runtime.spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(answer(stream, Arc::clone(&served)));
            }
        });
        Server {
            address,
            files,
            _runtime: runtime,
        }
    }

    /// Lists `package`, of `id` at `version`, as the only program of `waos/index.json`, and
    /// serves it beside the index.
    pub(crate) fn publish(&self, id: &str, package: &Path, version: &str) {
        let bytes = std::fs::read(package).unwrap();
        let file_name = package.file_name().unwrap().to_string_lossy().into_owned();
        let index = Index {
            programs: vec![ListedProgram {
                id: id.parse().unwrap(),
                name: "Triangle".to_owned(),
                version: version.parse().unwrap(),
                package: file_name.clone(),
                sha256: Sha256::digest(&bytes).into(),
            }],
        };
        let mut files = self.files.lock().unwrap();
        files.insert(format!("waos/{file_name}"), bytes);
        files.insert("waos/index.json".to_owned(), index.to_json().into_bytes());
    }

    /// The URL of the file `path` on this server.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.address)
    }
}

/// Answers the one request `stream` carries, then closes it.
async fn answer(mut stream: TcpStream, files: Arc<Mutex<HashMap<String, Vec<u8>>>>) {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|end| end == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let request = String::from_utf8_lossy(&request);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let found = files
        .lock()
        .unwrap()
        .get(path.trim_start_matches('/'))
        .cloned();
    let (status, body) = match found {
        Some(body) => ("200 OK", body),
        None => ("404 Not Found", b"not found".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(&body).await;
    let _ = stream.shutdown().await;
}
