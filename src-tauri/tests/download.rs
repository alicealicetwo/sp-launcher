//! Integration tests for the downloader, against a real HTTP server.
//!
//! These cover the paths that are easy to get wrong and impossible to eyeball:
//! resume from a partial file, a server that ignores `Range`, checksum
//! rejection, and the skip-if-already-correct shortcut.

use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use sp_launcher_lib::download::{self, Events, FileState, Manifest, ManifestEntry, Progress};

// ---------------------------------------------------------------- harness ---

#[derive(Default)]
struct Recorder {
    files: Mutex<Vec<FileState>>,
    completed: Mutex<Option<String>>,
    cancelled: AtomicUsize,
}

impl Events for Recorder {
    fn file(&self, state: FileState) {
        self.files.lock().unwrap().push(state);
    }
    fn progress(&self, _p: Progress) {}
    fn complete(&self, version: String) {
        *self.completed.lock().unwrap() = Some(version);
    }
    fn cancelled(&self) {
        self.cancelled.fetch_add(1, Ordering::Relaxed);
    }
}

impl Recorder {
    fn kinds(&self) -> Vec<String> {
        self.files
            .lock()
            .unwrap()
            .iter()
            .map(|f| match f {
                FileState::Verified { .. } => "verified",
                FileState::Downloading { .. } => "downloading",
                FileState::Done { .. } => "done",
                FileState::Failed { .. } => "failed",
            })
            .map(String::from)
            .collect()
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Minimal HTTP/1.1 server. `honour_range` off simulates hosting that ignores
/// `Range` and returns the whole body with 200.
async fn serve(
    body: Vec<u8>,
    honour_range: bool,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sent = Arc::new(AtomicUsize::new(0));
    let sent_outer = sent.clone();

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            let sent = sent.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();

                let start = if honour_range {
                    req.lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|l| l.split('=').nth(1).map(|s| s.trim().trim_end_matches('-').to_string()))
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                } else {
                    0
                };

                let slice = &body[start.min(body.len())..];
                let head = if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                        slice.len(), start, body.len() - 1, body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                        slice.len()
                    )
                };
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(slice).await;
                let _ = sock.flush().await;
                sent.fetch_add(slice.len(), Ordering::Relaxed);
            });
        }
    });

    (format!("http://127.0.0.1:{port}"), sent_outer, handle)
}

fn manifest_for(url: &str, rel: &str, body: &[u8]) -> Manifest {
    Manifest {
        version: "1.0.0".into(),
        files: vec![ManifestEntry {
            path: rel.into(),
            url: format!("{url}/{rel}"),
            size: body.len() as u64,
            sha256: sha256(body),
        }],
    }
}

// ------------------------------------------------------------------ tests ---

#[tokio::test]
async fn downloads_and_verifies_a_file() {
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (url, sent, server) = serve(body.clone(), true).await;
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Recorder::default());

    download::run(
        events.clone(),
        manifest_for(&url, "Content/x.pak", &body),
        dir.path().to_path_buf(),
        Arc::new(AtomicBool::new(false)),
        2,
    )
    .await
    .expect("download succeeds");

    let written = tokio::fs::read(dir.path().join("Content/x.pak")).await.unwrap();
    assert_eq!(written, body, "file on disk matches what the server sent");
    assert_eq!(events.kinds(), vec!["downloading", "done"]);
    assert_eq!(*events.completed.lock().unwrap(), Some("1.0.0".into()));
    // the .part must not survive a successful run
    assert!(!dir.path().join("Content/x.pak.part").exists());
    assert_eq!(sent.load(Ordering::Relaxed), body.len());
    server.abort();
}

#[tokio::test]
async fn resumes_from_a_partial_file() {
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 241) as u8).collect();
    let (url, sent, server) = serve(body.clone(), true).await;
    let dir = tempfile::tempdir().unwrap();

    // Simulate an interrupted run: the first 120 KB are already on disk.
    let cut = 120_000;
    tokio::fs::create_dir_all(dir.path().join("Content")).await.unwrap();
    tokio::fs::write(dir.path().join("Content/x.pak.part"), &body[..cut])
        .await
        .unwrap();

    let events = Arc::new(Recorder::default());
    download::run(
        events.clone(),
        manifest_for(&url, "Content/x.pak", &body),
        dir.path().to_path_buf(),
        Arc::new(AtomicBool::new(false)),
        1,
    )
    .await
    .expect("resumed download succeeds");

    let written = tokio::fs::read(dir.path().join("Content/x.pak")).await.unwrap();
    assert_eq!(written.len(), body.len());
    assert_eq!(written, body, "resumed file is byte-identical, not concatenated");
    assert_eq!(events.kinds(), vec!["downloading", "done"]);

    // The point of the test: only the missing tail crossed the wire. Without
    // this the test would still pass if resume silently fell back to a full
    // re-download, since the resulting file is identical either way.
    let served = sent.load(Ordering::Relaxed);
    assert_eq!(
        served,
        body.len() - cut,
        "expected only the remaining {} bytes to be sent, got {served}",
        body.len() - cut
    );
    server.abort();
}

#[tokio::test]
async fn restarts_when_the_server_ignores_range() {
    let body: Vec<u8> = (0..150_000u32).map(|i| (i % 199) as u8).collect();
    // honour_range = false: always replies 200 with the whole body.
    let (url, sent, server) = serve(body.clone(), false).await;
    let dir = tempfile::tempdir().unwrap();

    let cut = 60_000;
    tokio::fs::create_dir_all(dir.path().join("Content")).await.unwrap();
    tokio::fs::write(dir.path().join("Content/x.pak.part"), &body[..cut])
        .await
        .unwrap();

    let events = Arc::new(Recorder::default());
    download::run(
        events.clone(),
        manifest_for(&url, "Content/x.pak", &body),
        dir.path().to_path_buf(),
        Arc::new(AtomicBool::new(false)),
        1,
    )
    .await
    .expect("falls back to a fresh download");

    let written = tokio::fs::read(dir.path().join("Content/x.pak")).await.unwrap();
    assert_eq!(
        written, body,
        "must discard the partial rather than append a full body to it"
    );
    assert_eq!(
        sent.load(Ordering::Relaxed),
        body.len(),
        "the whole body is re-sent when the server cannot resume"
    );
    server.abort();
}

#[tokio::test]
async fn rejects_a_checksum_mismatch() {
    let body: Vec<u8> = (0..50_000u32).map(|i| (i % 97) as u8).collect();
    let (url, sent, server) = serve(body.clone(), true).await;
    let dir = tempfile::tempdir().unwrap();

    let mut manifest = manifest_for(&url, "Content/x.pak", &body);
    manifest.files[0].sha256 = sha256(b"something else entirely");

    let events = Arc::new(Recorder::default());
    let result = download::run(
        events.clone(),
        manifest,
        dir.path().to_path_buf(),
        Arc::new(AtomicBool::new(false)),
        1,
    )
    .await;

    let _ = sent;
    assert!(result.is_err(), "a bad hash must fail the run");
    assert!(events.kinds().contains(&"failed".to_string()));
    assert!(
        !dir.path().join("Content/x.pak").exists(),
        "a file that failed verification must never be committed"
    );
    assert!(
        !dir.path().join("Content/x.pak.part").exists(),
        "the bad partial is cleaned up so the next run starts fresh"
    );
    server.abort();
}

#[tokio::test]
async fn skips_a_file_that_is_already_correct() {
    let body: Vec<u8> = (0..80_000u32).map(|i| (i % 131) as u8).collect();
    let (url, sent, server) = serve(body.clone(), true).await;
    let dir = tempfile::tempdir().unwrap();

    tokio::fs::create_dir_all(dir.path().join("Content")).await.unwrap();
    tokio::fs::write(dir.path().join("Content/x.pak"), &body).await.unwrap();

    let events = Arc::new(Recorder::default());
    download::run(
        events.clone(),
        manifest_for(&url, "Content/x.pak", &body),
        dir.path().to_path_buf(),
        Arc::new(AtomicBool::new(false)),
        1,
    )
    .await
    .expect("verify pass succeeds");

    assert_eq!(
        events.kinds(),
        vec!["verified"],
        "an already-correct file is verified, never re-downloaded"
    );
    assert_eq!(
        sent.load(Ordering::Relaxed),
        0,
        "a verified file must not cost any bandwidth"
    );
    server.abort();
}

#[tokio::test]
async fn refuses_a_manifest_path_that_escapes_the_install_dir() {
    let body = b"payload".to_vec();
    let (url, sent, server) = serve(body.clone(), true).await;
    let dir = tempfile::tempdir().unwrap();

    let mut manifest = manifest_for(&url, "ok.bin", &body);
    manifest.files[0].path = "../escaped.bin".into();

    let events = Arc::new(Recorder::default());
    let result = download::run(
        events,
        manifest,
        dir.path().to_path_buf(),
        Arc::new(AtomicBool::new(false)),
        1,
    )
    .await;

    let _ = sent;
    assert!(result.is_err(), "traversal must be refused");
    assert!(!dir.path().parent().unwrap().join("escaped.bin").exists());
    server.abort();
}
