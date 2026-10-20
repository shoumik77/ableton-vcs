//! Snapshot manifests: the unit of history `daw` stores.
//!
//! Milestone 3 turns the (path, hash) pairs from milestone 2 into a persisted
//! record: `daw snapshot` captures the working tree into a `Manifest` —
//! timestamp, optional message, sorted file entries — written as JSON under
//! `.daw/snapshots/`. Status and diff (later milestones) are pure comparisons
//! of two manifests, so this format is the thing that must stay stable.

use crate::hash::{hash_project, FileHash};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// One file's identity at snapshot time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Root-relative path with `/` separators, so a manifest written on one
    /// OS stays readable on another.
    pub path: String,
    /// Serializes as a 64-digit hex string (see `FileHash`'s serde impls).
    pub hash: FileHash,
}

/// A named point in a project's history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// Sequence number within the repo, zero-padded ("0001") so that
    /// lexicographic filename order is chronological order.
    pub id: String,
    /// Seconds since the unix epoch — enough for ordering and display
    /// without pulling in a datetime crate.
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Sorted by path; `hash_project` already guarantees this order.
    pub files: Vec<FileEntry>,
}

impl Manifest {
    /// Hashes every project file under `root` into a new manifest.
    ///
    /// The caller supplies the `id` — the repo knows the next sequence
    /// number; this function only knows the working tree.
    pub fn capture(root: &Path, id: String, message: Option<String>) -> io::Result<Self> {
        let files = hash_project(root)?
            .into_iter()
            .map(|(path, hash)| FileEntry {
                path: path_to_forward_slashes(&path),
                hash,
            })
            .collect();

        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Manifest {
            id,
            created_at,
            message,
            files,
        })
    }
}

/// Serializes a path with `/` separators regardless of platform.
fn path_to_forward_slashes(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestProject {
        root: PathBuf,
    }

    impl TestProject {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir().join(format!(
                "daw-manifest-test-{}-{}",
                std::process::id(),
                id
            ));
            fs::create_dir_all(&root).unwrap();
            TestProject { root }
        }

        fn write(&self, rel: &str, contents: &[u8]) {
            let path = self.root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            File::create(path).unwrap().write_all(contents).unwrap();
        }
    }

    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn capture_pairs_every_scanned_file() {
        let project = TestProject::new();
        project.write("Project.als", b"<ableton/>");
        project.write("Samples/Sub/hat.wav", b"audio");

        let m = Manifest::capture(&project.root, "0001".into(), Some("msg".into())).unwrap();

        assert_eq!(m.id, "0001");
        assert_eq!(m.message.as_deref(), Some("msg"));
        assert_eq!(m.files.len(), 2);
        assert_eq!(m.files[0].path, "Project.als");
        assert_eq!(m.files[1].path, "Samples/Sub/hat.wav");
    }

    #[test]
    fn manifest_json_round_trip() {
        let project = TestProject::new();
        project.write("Project.als", b"v1");

        let m = Manifest::capture(&project.root, "0001".into(), None).unwrap();
        let json = serde_json::to_string_pretty(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, m.id);
        assert_eq!(back.created_at, m.created_at);
        assert_eq!(back.message, m.message);
        assert_eq!(back.files, m.files);
    }

    #[test]
    fn hash_serializes_as_hex_string() {
        // The manifest's JSON should look like sha256sum output, not an
        // array of numbers.
        let project = TestProject::new();
        project.write("Project.als", b"x");
        let m = Manifest::capture(&project.root, "0001".into(), None).unwrap();

        let json = serde_json::to_string(&m).unwrap();

        assert!(json.contains(
            "\"hash\":\"2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881\""
        ));
    }
}
