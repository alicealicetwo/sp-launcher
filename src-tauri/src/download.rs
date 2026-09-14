//! Game file download: resumable, parallel, hash-verified.
//!
//! The manifest lists every file with a size and a SHA-256. Each file streams
//! to a `.part` sibling and is hashed as it streams; it is only renamed into
//! place once the hash matches, so an interrupted run never leaves a truncated
//! file that looks complete. A file already on disk with the right hash is
//! skipped, which makes a re-run double as the "verify files" pass.
//!
//! For a ~28 GB install the two things that matter are resume (a dropped
//! connection must not cost you the whole file) and parallelism (a single
//! stream rarely saturates a connection). Both are here, and both are covered
//! by the integration tests in `tests/download.rs`.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::error::{LauncherError, Result};

/// Read-back size when resuming: the existing `.part` bytes have to go through
/// the hasher before appending, or the final digest is wrong.
const REHASH_CHUNK: usize = 1024 * 1024;

/// Where progress goes. The app implements this over `AppHandle::emit`; the
/// tests implement it over a Vec. Keeping the transfer logic free of Tauri is
/// what makes the resume path testable at all.
pub trait Events: Send + Sync {
    fn file(&self, state: FileState);
    fn progress(&self, progress: Progress);
    fn complete(&self, version: String);
    fn cancelled(&self);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Path relative to the install directory, forward slashes.
    pub path: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the finished file.
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub files: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    /// Most recently started file, for the "what is it doing" line.
    pub file: String,
    pub files_done: usize,
    pub file_count: usize,
    pub received: u64,
    pub total: u64,
    pub bytes_per_sec: f64,
    pub percent: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileState {
    Verified { path: String },
    Downloading { path: String },
    Done { path: String },
    Failed { path: String, error: String },
}

/// Reject anything that would escape the install directory: absolute paths,
/// drive letters and `..` all resolve to a refusal.
fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(LauncherError::Message(format!(
            "absolute path in manifest: {rel}"
        )));
    }
    let mut out = root.to_path_buf();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(LauncherError::Message(format!(
                    "manifest path escapes the install directory: {rel}"
                )));
            }
        }
    }
    Ok(out)
}

/// `.with_extension("part")` would turn `a.pak` into `a.part` — replacing the
/// extension, not appending — and collide with a real file of that name.
fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// Hash a file on disk, streaming so a 5 GB pak does not land in RAM.
async fn sha256_file(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; REHASH_CHUNK];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Shared counters, so the progress ticker can read totals without
/// coordinating with the workers.
struct Shared {
    received: AtomicU64,
    files_done: AtomicUsize,
    current: Mutex<String>,
    cancel: Arc<AtomicBool>,
}

async fn fetch_one(
    client: &reqwest::Client,
    events: &dyn Events,
    entry: &ManifestEntry,
    install_dir: &Path,
    shared: &Shared,
) -> Result<()> {
    let dest = safe_join(install_dir, &entry.path)?;
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Already correct on disk? Skip the transfer.
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
        if let Ok(existing) = sha256_file(&dest).await {
            if existing.eq_ignore_ascii_case(&entry.sha256) {
                shared.received.fetch_add(entry.size, Ordering::Relaxed);
                shared.files_done.fetch_add(1, Ordering::Relaxed);
                events.file(FileState::Verified {
                    path: entry.path.clone(),
                });
                return Ok(());
            }
        }
    }

    *shared.current.lock().expect("current mutex") = entry.path.clone();
    events.file(FileState::Downloading {
        path: entry.path.clone(),
    });

    let part = part_path(&dest);
    let mut hasher = Sha256::new();

    // Resume: feed whatever is already in the .part through the hasher and ask
    // the server to continue from there.
    let mut have: u64 = match tokio::fs::metadata(&part).await {
        Ok(m) if m.is_file() => m.len(),
        _ => 0,
    };
    if have > entry.size {
        // A .part longer than the target means the manifest changed under us.
        have = 0;
        let _ = tokio::fs::remove_file(&part).await;
    }
    if have > 0 {
        let mut existing = tokio::fs::File::open(&part).await?;
        let mut buf = vec![0u8; REHASH_CHUNK];
        let mut read_total = 0u64;
        loop {
            let n = existing.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            read_total += n as u64;
        }
        have = read_total;
        shared.received.fetch_add(have, Ordering::Relaxed);
    }

    let mut request = client.get(&entry.url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let response = request.send().await?.error_for_status()?;

    // If the server ignored the range and sent the whole body, start over
    // rather than concatenating and producing a corrupt file.
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if have > 0 && !resumed {
        shared.received.fetch_sub(have, Ordering::Relaxed);
        hasher = Sha256::new();
        have = 0;
    }

    let mut file = if have > 0 {
        let mut f = tokio::fs::OpenOptions::new().write(true).open(&part).await?;
        f.seek(std::io::SeekFrom::Start(have)).await?;
        f
    } else {
        tokio::fs::File::create(&part).await?
    };

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if shared.cancel.load(Ordering::Relaxed) {
            file.flush().await?;
            // Leave the .part in place: the next run resumes from it.
            return Ok(());
        }
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        shared
            .received
            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }

    file.flush().await?;
    drop(file);

    let got = hex::encode(hasher.finalize());
    if !got.eq_ignore_ascii_case(&entry.sha256) {
        let _ = tokio::fs::remove_file(&part).await;
        let msg = format!(
            "checksum mismatch for {} (expected {}, got {})",
            entry.path, entry.sha256, got
        );
        events.file(FileState::Failed {
            path: entry.path.clone(),
            error: msg.clone(),
        });
        return Err(LauncherError::Message(msg));
    }

    // Windows will not rename onto an existing file.
    let _ = tokio::fs::remove_file(&dest).await;
    tokio::fs::rename(&part, &dest).await?;

    shared.files_done.fetch_add(1, Ordering::Relaxed);
    events.file(FileState::Done {
        path: entry.path.clone(),
    });
    Ok(())
}

pub async fn run(
    events: Arc<dyn Events>,
    manifest: Manifest,
    install_dir: PathBuf,
    cancel: Arc<AtomicBool>,
    concurrency: usize,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("SPLauncher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .build()?;

    let total_bytes: u64 = manifest.files.iter().map(|f| f.size).sum();
    let file_count = manifest.files.len();

    let shared = Arc::new(Shared {
        received: AtomicU64::new(0),
        files_done: AtomicUsize::new(0),
        current: Mutex::new(String::new()),
        cancel: cancel.clone(),
    });

    // One ticker owns progress reporting. Emitting from every worker would
    // either flood the IPC channel or need its own throttle per worker.
    let ticker = {
        let events = events.clone();
        let shared = shared.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            let mut interval = tokio::time::interval(Duration::from_millis(140));
            loop {
                interval.tick().await;
                if shared.cancel.load(Ordering::Relaxed) {
                    break;
                }
                let received = shared.received.load(Ordering::Relaxed);
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                events.progress(Progress {
                    file: shared.current.lock().expect("current mutex").clone(),
                    files_done: shared.files_done.load(Ordering::Relaxed),
                    file_count,
                    received,
                    total: total_bytes,
                    bytes_per_sec: received as f64 / elapsed,
                    percent: if total_bytes > 0 {
                        (received as f64 / total_bytes as f64 * 100.0).min(100.0)
                    } else {
                        0.0
                    },
                });
                if received >= total_bytes && total_bytes > 0 {
                    break;
                }
            }
        })
    };

    let limit = concurrency.clamp(1, 16);
    let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    futures_util::stream::iter(manifest.files.iter())
        .for_each_concurrent(limit, |entry| {
            let client = client.clone();
            let events = events.clone();
            let shared = shared.clone();
            let install_dir = install_dir.clone();
            let failure = failure.clone();
            async move {
                if shared.cancel.load(Ordering::Relaxed) {
                    return;
                }
                // One bad file stops the run, but whatever is already in
                // flight is allowed to finish rather than being torn down
                // mid-write.
                if failure.lock().expect("failure mutex").is_some() {
                    return;
                }
                if let Err(e) =
                    fetch_one(&client, events.as_ref(), entry, &install_dir, &shared).await
                {
                    *failure.lock().expect("failure mutex") = Some(e.to_string());
                    shared.cancel.store(true, Ordering::Relaxed);
                }
            }
        })
        .await;

    ticker.abort();

    if let Some(msg) = failure.lock().expect("failure mutex").clone() {
        return Err(LauncherError::Message(msg));
    }
    if cancel.load(Ordering::Relaxed) {
        events.cancelled();
        return Ok(());
    }

    events.complete(manifest.version.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{part_path, safe_join};
    use std::path::Path;

    #[test]
    fn part_path_appends_rather_than_replacing_extension() {
        assert_eq!(
            part_path(Path::new("/game/a.pak")),
            Path::new("/game/a.pak.part")
        );
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(safe_join(Path::new("/game"), "../etc/passwd").is_err());
        assert!(safe_join(Path::new("/game"), "a/../../b").is_err());
    }

    #[test]
    fn accepts_normal_nested_paths() {
        assert_eq!(
            safe_join(Path::new("/game"), "Content/Paks/x.pak").unwrap(),
            Path::new("/game/Content/Paks/x.pak")
        );
    }
}
