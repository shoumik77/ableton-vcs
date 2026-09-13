//! Project scanner: discovers which files make up an Ableton project.
//!
//! Milestone 1 keeps the rules deliberately small:
//!   - top-level `*.als` files (the Live set itself)
//!   - everything under `Samples/` (recorded/imported audio)
//!
//! Everything else (`Backup/`, `Ableton Project Info/`, `.daw/`, stray files)
//! is ignored. Filtering rules can grow later if real projects demand it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Returns the sorted list of project files, as paths relative to `root`.
///
/// Relative paths are used so a project moved to a different folder still
/// produces the same snapshot. Sorting keeps the output deterministic, which
/// later milestones rely on when hashing and diffing manifests.
pub fn scan_project(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_file() && has_als_extension(&path) {
            files.push(path);
        } else if file_type.is_dir() && entry.file_name() == "Samples" {
            collect_files_recursive(&path, &mut files)?;
        }
    }

    // Convert absolute paths to root-relative, then sort for determinism.
    let mut relative = Vec::with_capacity(files.len());
    for path in files {
        // strip_prefix can only fail if `path` doesn't start with `root`,
        // which can't happen here since every path came from read_dir(root).
        relative.push(path.strip_prefix(root).unwrap().to_path_buf());
    }
    relative.sort();

    Ok(relative)
}

/// Recursively appends every file under `dir` to `out`.
///
/// `entry.file_type()` does not follow symlinks, so a symlinked directory is
/// treated as a file-like leaf rather than traversed — this avoids infinite
/// loops on symlink cycles.
fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_files_recursive(&path, out)?;
        } else if file_type.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn has_als_extension(path: &Path) -> bool {
    // Path::extension() returns Option<&OsStr>: None when the file has no
    // extension. Comparing an &OsStr to "als" works because OsStr implements
    // PartialEq<str>.
    path.extension() == Some(std::ffi::OsStr::new("als"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Each test gets its own directory under the system temp dir so tests can
    // run in parallel without colliding. No `tempfile` crate needed.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestProject {
        root: PathBuf,
    }

    impl TestProject {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir().join(format!(
                "daw-scanner-test-{}-{}",
                std::process::id(),
                id
            ));
            fs::create_dir_all(&root).unwrap();
            TestProject { root }
        }

        fn touch(&self, rel: &str) {
            let path = self.root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            File::create(path).unwrap();
        }
    }

    // Clean up the temp dir when the test finishes, even if it panics.
    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn finds_als_and_samples() {
        let project = TestProject::new();
        project.touch("Project.als");
        project.touch("Samples/kick.wav");
        project.touch("Samples/Sub/hat.wav");

        let files = scan_project(&project.root).unwrap();

        assert_eq!(
            files,
            vec![
                PathBuf::from("Project.als"),
                PathBuf::from("Samples/Sub/hat.wav"),
                PathBuf::from("Samples/kick.wav"),
            ]
        );
    }

    #[test]
    fn ignores_backup_and_project_info() {
        let project = TestProject::new();
        project.touch("Project.als");
        project.touch("Backup/Project [2024-01-01].als");
        project.touch("Ableton Project Info/Project8_1");
        project.touch("notes.txt");

        let files = scan_project(&project.root).unwrap();

        assert_eq!(files, vec![PathBuf::from("Project.als")]);
    }

    #[test]
    fn output_is_sorted() {
        let project = TestProject::new();
        // Created in non-alphabetical order; result must still be sorted.
        project.touch("Samples/zeta.wav");
        project.touch("Samples/alpha.wav");
        project.touch("Zebra.als");
        project.touch("Alpha.als");

        let files = scan_project(&project.root).unwrap();
        let mut sorted = files.clone();
        sorted.sort();

        assert_eq!(files, sorted);
    }

    #[test]
    fn empty_project_returns_empty_vec() {
        let project = TestProject::new();
        project.touch("unrelated.txt");

        let files = scan_project(&project.root).unwrap();

        assert!(files.is_empty());
    }

    #[test]
    fn missing_root_is_error() {
        let bogus = std::env::temp_dir().join("daw-scanner-test-does-not-exist");

        let result = scan_project(&bogus);

        assert!(result.is_err());
    }
}
