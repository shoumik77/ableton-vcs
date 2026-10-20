//! Content hashing: gives every project file a SHA-256 fingerprint.
//!
//! Milestone 2 builds on the scanner: `hash_project` produces a sorted list
//! of (path, hash) pairs — the raw material a snapshot/manifest is made of.
//! Two snapshots of the same project can later be compared purely by these
//! pairs, without re-reading any file contents.

use crate::scanner::scan_project;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

/// A SHA-256 content hash, stored as raw bytes.
///
/// Kept as a newtype rather than a bare `[u8; 32]` so hex formatting and
/// (later) parsing live in one place.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileHash([u8; 32]);

impl fmt::Display for FileHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for FileHash {
    // Debug prints the same hex as Display: `abc123...` instead of a raw
    // byte array, which keeps assert failure messages readable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Computes the SHA-256 of the file at `path`.
///
/// The file is streamed through a BufReader rather than loaded whole, so
/// multi-hundred-MB audio files hash in constant memory.
pub fn hash_file(path: &Path) -> io::Result<FileHash> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();

    // Sha256 implements io::Write, so io::copy pumps bytes straight into it.
    io::copy(&mut reader, &mut hasher)?;

    Ok(FileHash(hasher.finalize().into()))
}

/// Scans `root` and hashes every file it finds.
///
/// Returns sorted (relative path, hash) pairs — same ordering guarantee as
/// `scan_project`, so the output is deterministic.
pub fn hash_project(root: &Path) -> io::Result<Vec<(PathBuf, FileHash)>> {
    let files = scan_project(root)?;

    let mut hashed = Vec::with_capacity(files.len());
    for rel_path in files {
        let hash = hash_file(&root.join(&rel_path))?;
        hashed.push((rel_path, hash));
    }

    Ok(hashed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Same temp-dir pattern as the scanner tests: unique dir per test,
    // cleaned up on drop even if the test panics.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestProject {
        root: PathBuf,
    }

    impl TestProject {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir().join(format!(
                "daw-hash-test-{}-{}",
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
    fn identical_contents_identical_hash() {
        let project = TestProject::new();
        project.write("a.txt", b"same bytes");
        project.write("b.txt", b"same bytes");

        let hash_a = hash_file(&project.root.join("a.txt")).unwrap();
        let hash_b = hash_file(&project.root.join("b.txt")).unwrap();

        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn changed_contents_changed_hash() {
        let project = TestProject::new();
        project.write("f.txt", b"version one");
        let before = hash_file(&project.root.join("f.txt")).unwrap();

        project.write("f.txt", b"version two");
        let after = hash_file(&project.root.join("f.txt")).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn matches_known_sha256_vector() {
        // SHA-256("hello") is a fixed public constant — if we compute
        // anything else, the hashing itself is wrong.
        let project = TestProject::new();
        project.write("hello.txt", b"hello");

        let hash = hash_file(&project.root.join("hello.txt")).unwrap();

        assert_eq!(
            hash.to_string(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn empty_file_hashes() {
        // Edge case: a 0-byte file must still hash (to the SHA-256 of
        // nothing) rather than error.
        let project = TestProject::new();
        project.write("empty.txt", b"");

        let hash = hash_file(&project.root.join("empty.txt")).unwrap();

        assert_eq!(
            hash.to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hash_project_pairs_every_scanned_file() {
        let project = TestProject::new();
        project.write("Project.als", b"<ableton/>");
        project.write("Samples/kick.wav", b"audio bytes");
        project.write("Backup/ignored.als", b"should not appear");

        let entries = hash_project(&project.root).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, PathBuf::from("Project.als"));
        assert_eq!(entries[1].0, PathBuf::from("Samples/kick.wav"));
        // Spot-check that hashes are real, not placeholders.
        assert_eq!(
            entries[0].1,
            hash_file(&project.root.join("Project.als")).unwrap()
        );
    }

    #[test]
    fn hash_project_detects_change() {
        // The core operation future `status` depends on: hash the project,
        // change a file, hash again — exactly one entry differs.
        let project = TestProject::new();
        project.write("Project.als", b"v1");
        project.write("Samples/kick.wav", b"audio");

        let before = hash_project(&project.root).unwrap();
        project.write("Samples/kick.wav", b"edited audio");
        let after = hash_project(&project.root).unwrap();

        assert_eq!(before.len(), after.len());
        for (old, new) in before.iter().zip(after.iter()) {
            assert_eq!(old.0, new.0); // same paths
            if old.0 == PathBuf::from("Samples/kick.wav") {
                assert_ne!(old.1, new.1); // changed file changed hash
            } else {
                assert_eq!(old.1, new.1); // untouched file kept hash
            }
        }
    }
}
