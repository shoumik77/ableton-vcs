//! Manifest comparison: the read side of history.
//!
//! Milestone 4 turns two `Manifest`s — a snapshot vs the working tree, or
//! two snapshots — into a `Diff`: a path-sorted list of added / modified /
//! deleted entries. Because `Manifest::files` is always sorted by path, the
//! comparison is a single two-index merge walk; no maps, no allocations
//! beyond the result.

use crate::manifest::Manifest;
use std::cmp::Ordering;
use std::fmt;

/// How one path differs between two manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Present in `to` but not in `from`.
    Added { path: String },
    /// Present in both, but the content hash differs.
    Modified { path: String },
    /// Present in `from` but not in `to`.
    Deleted { path: String },
}

impl fmt::Display for Change {
    // git-style short status: `A  path`, `M  path`, `D  path`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (tag, path) = match self {
            Change::Added { path } => ("A", path),
            Change::Modified { path } => ("M", path),
            Change::Deleted { path } => ("D", path),
        };
        write!(f, "{tag}  {path}")
    }
}

/// The result of comparing two manifests.
///
/// `changes` is sorted by path, which makes the output deterministic and
/// groups every kind of change to one file into a single entry.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub changes: Vec<Change>,
}

impl Diff {
    /// True when the two manifests agree on every path.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Compares `from` (older) to `to` (newer) and returns what changed.
///
/// Both manifests' `files` lists are sorted by path, so this walks them in
/// lock-step: the side with the smaller path is the one that differs.
pub fn diff_manifests(from: &Manifest, to: &Manifest) -> Diff {
    let mut diff = Diff::default();
    let mut i = 0;
    let mut j = 0;

    while i < from.files.len() || j < to.files.len() {
        match (from.files.get(i), to.files.get(j)) {
            (Some(a), Some(b)) => match a.path.cmp(&b.path) {
                Ordering::Less => {
                    diff.changes.push(Change::Deleted {
                        path: a.path.clone(),
                    });
                    i += 1;
                }
                Ordering::Greater => {
                    diff.changes.push(Change::Added {
                        path: b.path.clone(),
                    });
                    j += 1;
                }
                Ordering::Equal => {
                    if a.hash != b.hash {
                        diff.changes.push(Change::Modified {
                            path: a.path.clone(),
                        });
                    }
                    i += 1;
                    j += 1;
                }
            },
            (Some(a), None) => {
                diff.changes.push(Change::Deleted {
                    path: a.path.clone(),
                });
                i += 1;
            }
            (None, Some(b)) => {
                diff.changes.push(Change::Added {
                    path: b.path.clone(),
                });
                j += 1;
            }
            (None, None) => break,
        }
    }

    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::FileEntry;

    // Builds a manifest from (path, hash-seed) pairs: each "hash" is a
    // distinct real FileHash derived from the seed byte so tests never
    // touch the filesystem.
    fn manifest(pairs: &[(&str, u8)]) -> Manifest {
        let files = pairs
            .iter()
            .map(|(path, seed)| FileEntry {
                path: (*path).to_string(),
                hash: format!("{:0>64}", format!("{:x}", seed)).parse().unwrap(),
            })
            .collect();
        Manifest {
            id: "0001".into(),
            created_at: 0,
            message: None,
            files,
        }
    }

    fn paths(diff: &Diff) -> Vec<String> {
        diff.changes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn identical_manifests_produce_empty_diff() {
        let a = manifest(&[("a.txt", 1), ("b.txt", 2)]);
        let b = manifest(&[("a.txt", 1), ("b.txt", 2)]);

        let diff = diff_manifests(&a, &b);

        assert!(diff.is_empty());
    }

    #[test]
    fn file_only_in_to_is_added() {
        let a = manifest(&[("a.txt", 1)]);
        let b = manifest(&[("a.txt", 1), ("new.txt", 3)]);

        let diff = diff_manifests(&a, &b);

        assert_eq!(paths(&diff), vec!["A  new.txt"]);
    }

    #[test]
    fn same_path_different_hash_is_modified() {
        let a = manifest(&[("a.txt", 1)]);
        let b = manifest(&[("a.txt", 9)]);

        let diff = diff_manifests(&a, &b);

        assert_eq!(paths(&diff), vec!["M  a.txt"]);
    }

    #[test]
    fn file_only_in_from_is_deleted() {
        let a = manifest(&[("a.txt", 1), ("gone.txt", 2)]);
        let b = manifest(&[("a.txt", 1)]);

        let diff = diff_manifests(&a, &b);

        assert_eq!(paths(&diff), vec!["D  gone.txt"]);
    }

    #[test]
    fn mixed_changes_are_sorted_by_path() {
        let a = manifest(&[("b.txt", 1), ("c.txt", 1), ("d.txt", 1)]);
        let b = manifest(&[("a.txt", 1), ("b.txt", 9), ("d.txt", 1)]);

        let diff = diff_manifests(&a, &b);

        assert_eq!(paths(&diff), vec!["A  a.txt", "M  b.txt", "D  c.txt"]);
    }

    #[test]
    fn empty_from_marks_everything_added() {
        let a = manifest(&[]);
        let b = manifest(&[("x.txt", 1), ("y.txt", 2)]);

        let diff = diff_manifests(&a, &b);

        assert_eq!(paths(&diff), vec!["A  x.txt", "A  y.txt"]);
    }
}
