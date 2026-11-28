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

/// Where a website keeps what it lists, after its address: the folder a source typed by its
/// address alone is read in.
const SOURCE_FOLDER: &str = ".well-known/waos/";

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

    /// Lists `package`, of `id` at `version`, as the only program of the index of the source the
    /// server is ([`source`](Self::source)), and serves it beside the index.
    pub(crate) fn publish(&self, id: &str, package: &Path, version: &str) {
        self.publish_saying(id, package, version, |_| {});
    }

    /// [`publish`](Self::publish), the index saying of itself and of the program what `say`
    /// adds: the source's name, what it puts forward, what a store shows of the program.
    pub(crate) fn publish_saying(
        &self,
        id: &str,
        package: &Path,
        version: &str,
        say: impl FnOnce(&mut Index),
    ) {
        let (file_name, bytes) = file_of(package);
        let mut index = Index {
            programs: vec![listed(id, version, &file_name, &bytes)],
            ..Index::default()
        };
        say(&mut index);
        let mut files = self.files.lock().unwrap();
        files.insert(format!("{SOURCE_FOLDER}{file_name}"), bytes);
        files.insert(
            format!("{SOURCE_FOLDER}index.json"),
            index.to_json().into_bytes(),
        );
    }

    /// Lists each of `packages`, an id, its package and its version, in the index of the source
    /// the server is, and serves each beside the index.
    pub(crate) fn publish_each(&self, packages: &[(&str, &Path, &str)]) {
        let mut files = self.files.lock().unwrap();
        let mut index = Index::default();
        for (id, package, version) in packages {
            let (file_name, bytes) = file_of(package);
            index.programs.push(listed(id, version, &file_name, &bytes));
            files.insert(format!("{SOURCE_FOLDER}{file_name}"), bytes);
        }
        files.insert(
            format!("{SOURCE_FOLDER}index.json"),
            index.to_json().into_bytes(),
        );
    }

    /// Serves `bytes` as the file `name` beside the index, as a source's picture is.
    pub(crate) fn serve_beside_index(&self, name: &str, bytes: &[u8]) {
        let mut files = self.files.lock().unwrap();
        files.insert(format!("{SOURCE_FOLDER}{name}"), bytes.to_vec());
    }

    /// The server as a source, as its user types it: a website's address, whose apps are listed
    /// where a website's are.
    pub(crate) fn source(&self) -> String {
        format!("http://{}", self.address)
    }
}

/// The name of the file at `package`, and its bytes.
fn file_of(package: &Path) -> (String, Vec<u8>) {
    let bytes = std::fs::read(package).unwrap();
    let file_name = package.file_name().unwrap().to_string_lossy().into_owned();
    (file_name, bytes)
}

/// `id` at `version`, as an index lists it, in the file `file_name` of `bytes`.
fn listed(id: &str, version: &str, file_name: &str, bytes: &[u8]) -> ListedProgram {
    ListedProgram::new(
        id.parse().unwrap(),
        "Triangle",
        version.parse().unwrap(),
        file_name.to_owned(),
        Sha256::digest(bytes).into(),
    )
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
