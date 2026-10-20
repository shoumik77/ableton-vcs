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

use crate::diff::{diff_manifests, Diff};
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
    ///
    /// A missing id reports `NotFound` with the id in the message rather
    /// than the raw filesystem error, so `daw diff 0009` reads sensibly.
    pub fn read_snapshot(&self, id: &str) -> io::Result<Manifest> {
        let path = self.snapshots_dir().join(format!("{id}.json"));
        let json = fs::read_to_string(&path).map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                io::Error::new(ErrorKind::NotFound, format!("snapshot {id} not found"))
            } else {
                e
            }
        })?;
        serde_json::from_str(&json).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }

    /// The most recent snapshot, or `None` before the first one.
    pub fn latest_snapshot(&self) -> io::Result<Option<Manifest>> {
        match self.list_snapshots()?.last() {
            Some(id) => Ok(Some(self.read_snapshot(id)?)),
            None => Ok(None),
        }
    }

    /// What the working tree looks like relative to the latest snapshot
    /// (or to nothing, before the first snapshot).
    pub fn status(&self) -> io::Result<Diff> {
        let base = self.latest_snapshot()?.unwrap_or_else(Manifest::empty);
        let working = Manifest::capture(&self.root, "working".into(), None)?;
        Ok(diff_manifests(&base, &working))
    }

    /// Compares two stored snapshots, `a` as the older side.
    pub fn diff_snapshots(&self, a: &str, b: &str) -> io::Result<Diff> {
        Ok(diff_manifests(
            &self.read_snapshot(a)?,
            &self.read_snapshot(b)?,
        ))
    }

    /// Compares a stored snapshot to the current working tree.
    pub fn diff_snapshot_to_working(&self, a: &str) -> io::Result<Diff> {
        let working = Manifest::capture(&self.root, "working".into(), None)?;
        Ok(diff_manifests(&self.read_snapshot(a)?, &working))
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
    use std::fs::{self, File};
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

    fn change_lines(diff: &Diff) -> Vec<String> {
        diff.changes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn status_before_first_snapshot_marks_all_added() {
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        project.write("Samples/kick.wav", b"audio");
        let repo = Repo::init(&project.root).unwrap();

        let diff = repo.status().unwrap();

        assert_eq!(
            change_lines(&diff),
            vec!["A  Project.als", "A  Samples/kick.wav"]
        );
    }

    #[test]
    fn status_after_unchanged_snapshot_is_clean() {
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        let repo = Repo::init(&project.root).unwrap();
        repo.create_snapshot(None).unwrap();

        let diff = repo.status().unwrap();

        assert!(diff.is_empty());
    }

    #[test]
    fn status_reports_add_modify_delete() {
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        project.write("Samples/kick.wav", b"audio");
        project.write("Samples/hat.wav", b"hat");
        let repo = Repo::init(&project.root).unwrap();
        repo.create_snapshot(None).unwrap();

        project.write("Samples/kick.wav", b"edited");
        project.write("Samples/snare.wav", b"new");
        fs::remove_file(project.root.join("Samples/hat.wav")).unwrap();

        let diff = repo.status().unwrap();

        assert_eq!(
            change_lines(&diff),
            vec![
                "D  Samples/hat.wav",
                "M  Samples/kick.wav",
                "A  Samples/snare.wav"
            ]
        );
    }

    #[test]
    fn diff_snapshots_compares_two_stored_manifests() {
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        let repo = Repo::init(&project.root).unwrap();
        repo.create_snapshot(None).unwrap();
        project.write("Project.als", b"v2");
        repo.create_snapshot(None).unwrap();

        let diff = repo.diff_snapshots("0001", "0002").unwrap();

        assert_eq!(change_lines(&diff), vec!["M  Project.als"]);
    }

    #[test]
    fn read_snapshot_missing_id_is_not_found() {
        let project = TestProject::new();
        let repo = Repo::init(&project.root).unwrap();

        let err = repo.read_snapshot("0009").unwrap_err();

        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.to_string(), "snapshot 0009 not found");
    }
}
