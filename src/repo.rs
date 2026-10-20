//! The on-disk repository: a `.daw/` directory inside the project.
//!
//! Milestone 3 layout:
//!
//! ```text
//! <project>/
//! ├── Project.als
//! ├── Samples/…
//! └── .daw/
//!     └── snapshots/
//!         ├── 0001.json
//!         └── 0002.json
//! ```
//!
//! Snapshots are content manifests, not file copies. Restore support can add
//! a `blobs/` directory later without changing this layout. The scanner never
//! picks up `.daw/` itself — it only takes top-level `*.als` and `Samples/`.

use crate::manifest::Manifest;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// Name of the metadata directory `daw init` creates inside a project.
pub const DIR: &str = ".daw";
/// Subdirectory of `.daw/` holding one JSON manifest per snapshot.
const SNAPSHOTS: &str = "snapshots";

/// A project directory that `daw` is tracking.
#[derive(Debug)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Creates `.daw/` under `root` and returns the new repository.
    ///
    /// Errors with `AlreadyExists` when the project is already initialised —
    /// re-init must be explicit, never a silent no-op.
    pub fn init(root: &Path) -> io::Result<Self> {
        let repo = Self {
            root: root.to_path_buf(),
        };
        if repo.snapshots_dir().exists() {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!("{} is already initialised", root.display()),
            ));
        }
        fs::create_dir_all(repo.snapshots_dir())?;
        Ok(repo)
    }

    /// Opens the repository at `root`, requiring a prior `init`.
    pub fn open(root: &Path) -> io::Result<Self> {
        let repo = Self {
            root: root.to_path_buf(),
        };
        if !repo.snapshots_dir().is_dir() {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!(
                    "{} is not a daw repository (run `daw init` first)",
                    root.display()
                ),
            ));
        }
        Ok(repo)
    }

    /// Hashes the working tree and stores the result as the next snapshot.
    pub fn create_snapshot(&self, message: Option<String>) -> io::Result<Manifest> {
        let manifest = Manifest::capture(&self.root, self.next_snapshot_id()?, message)?;
        self.write_snapshot(&manifest)?;
        Ok(manifest)
    }

    /// Writes `manifest` as `<id>.json` under `.daw/snapshots/`.
    pub fn write_snapshot(&self, manifest: &Manifest) -> io::Result<PathBuf> {
        let json = serde_json::to_string_pretty(manifest)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
        let path = self.snapshots_dir().join(format!("{}.json", manifest.id));
        fs::write(&path, json)?;
        Ok(path)
    }

    /// Reads back the manifest stored as `.daw/snapshots/<id>.json`.
    pub fn read_snapshot(&self, id: &str) -> io::Result<Manifest> {
        let json = fs::read_to_string(self.snapshots_dir().join(format!("{id}.json")))?;
        serde_json::from_str(&json).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }

    /// Snapshot ids in chronological order (zero-padded ids sort correctly).
    pub fn list_snapshots(&self) -> io::Result<Vec<String>> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(self.snapshots_dir())? {
            let path = entry?.path();
            if path.extension() == Some(OsStr::new("json")) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(stem.to_string());
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// The id the next snapshot should get: one past the highest existing id.
    ///
    /// Zero-padded to 4 digits so filename sort order matches chronological
    /// order for the first ten thousand snapshots.
    fn next_snapshot_id(&self) -> io::Result<String> {
        let next = self
            .list_snapshots()?
            .last()
            .and_then(|id| id.parse::<u64>().ok())
            .map_or(1, |n| n + 1);
        Ok(format!("{next:04}"))
    }

    fn snapshots_dir(&self) -> PathBuf {
        self.root.join(DIR).join(SNAPSHOTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestProject {
        root: PathBuf,
    }

    impl TestProject {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root =
                std::env::temp_dir().join(format!("daw-repo-test-{}-{}", std::process::id(), id));
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
    fn init_creates_snapshots_dir() {
        let project = TestProject::new();

        Repo::init(&project.root).unwrap();

        assert!(project.root.join(".daw/snapshots").is_dir());
    }

    #[test]
    fn init_twice_errors() {
        let project = TestProject::new();
        Repo::init(&project.root).unwrap();

        let result = Repo::init(&project.root);

        assert_eq!(result.unwrap_err().kind(), ErrorKind::AlreadyExists);
    }

    #[test]
    fn open_uninitialised_errors() {
        let project = TestProject::new();

        let result = Repo::open(&project.root);

        assert_eq!(result.unwrap_err().kind(), ErrorKind::NotFound);
    }

    #[test]
    fn snapshots_get_sequential_ids() {
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        let repo = Repo::init(&project.root).unwrap();

        let first = repo.create_snapshot(None).unwrap();
        let second = repo.create_snapshot(Some("second".into())).unwrap();

        assert_eq!(first.id, "0001");
        assert_eq!(second.id, "0002");
        assert_eq!(repo.list_snapshots().unwrap(), vec!["0001", "0002"]);
    }

    #[test]
    fn snapshot_round_trips_through_disk() {
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        project.write("Samples/kick.wav", b"audio");
        let repo = Repo::init(&project.root).unwrap();

        let written = repo.create_snapshot(Some("first".into())).unwrap();
        let read = repo.read_snapshot(&written.id).unwrap();

        assert_eq!(read.id, written.id);
        assert_eq!(read.message.as_deref(), Some("first"));
        assert_eq!(read.files, written.files);
        assert!(project.root.join(".daw/snapshots/0001.json").is_file());
    }

    #[test]
    fn snapshot_excludes_daw_dir_itself() {
        // Regression guard: after a snapshot exists, `.daw/` must not leak
        // into the next snapshot's file list.
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        let repo = Repo::init(&project.root).unwrap();
        repo.create_snapshot(None).unwrap();

        let second = repo.create_snapshot(None).unwrap();

        assert_eq!(second.files.len(), 1);
        assert_eq!(second.files[0].path, "Project.als");
    }
}
