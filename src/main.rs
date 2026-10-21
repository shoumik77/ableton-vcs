use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// clap owns argument parsing; each cmd_* below maps a parsed subcommand to
// a repo operation and decides the exit code.
#[derive(Parser)]
#[command(name = "daw", about = "Version control for Ableton Live projects")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start tracking a project directory
    Init {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Record a manifest of the project
    Snapshot {
        #[arg(default_value = ".")]
        dir: PathBuf,
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Show added/modified/deleted files vs the latest snapshot
    Status {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Compare two snapshots, or a snapshot to the working tree
    Diff {
        a: String,
        b: Option<String>,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// List snapshots, newest first
    Log {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Debug: print hash + path for every scanned file
    Hash { dir: PathBuf },
    /// Restore the working tree to a snapshot
    Restore {
        id: String,
        /// Overwrite even if the working tree has unsnapshotted changes
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Init { dir } => cmd_init(&dir),
        Command::Snapshot { dir, message } => cmd_snapshot(&dir, message),
        Command::Status { dir } => cmd_status(&dir),
        Command::Diff { a, b, dir } => cmd_diff(&a, b.as_deref(), &dir),
        Command::Log { dir } => cmd_log(&dir),
        Command::Hash { dir } => cmd_hash(&dir),
        Command::Restore { id, force, dir } => cmd_restore(&id, force, &dir),
    }
}

fn cmd_init(dir: &Path) -> ExitCode {
    match daw::repo::Repo::init(dir) {
        Ok(_) => {
            println!("initialised {}", dir.join(daw::repo::DIR).display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_snapshot(dir: &Path, message: Option<String>) -> ExitCode {
    match daw::repo::Repo::open(dir).and_then(|r| r.create_snapshot(message)) {
        Ok(manifest) => {
            println!("snapshot {} — {} files", manifest.id, manifest.files.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_hash(dir: &Path) -> ExitCode {
    match daw::hash::hash_project(dir) {
        Ok(entries) => {
            for (path, hash) in entries {
                println!("{hash}  {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// `status`/`diff` return 1 when changes exist so scripts can branch on a
// dirty tree; `status` errors return 2, keeping them distinguishable.
fn print_diff(diff: &daw::diff::Diff, exit_on_changes: ExitCode) -> ExitCode {
    if diff.is_empty() {
        println!("clean");
        ExitCode::SUCCESS
    } else {
        for change in &diff.changes {
            println!("{change}");
        }
        exit_on_changes
    }
}

fn cmd_status(dir: &Path) -> ExitCode {
    match daw::repo::Repo::open(dir).and_then(|r| r.status()) {
        Ok(diff) => print_diff(&diff, ExitCode::from(1)),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_diff(a: &str, b: Option<&str>, dir: &Path) -> ExitCode {
    let result = daw::repo::Repo::open(dir).and_then(|repo| match b {
        Some(b) => repo.diff_snapshots(a, b),
        None => repo.diff_snapshot_to_working(a),
    });

    match result {
        Ok(diff) => print_diff(&diff, ExitCode::from(1)),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_log(dir: &Path) -> ExitCode {
    let result: std::io::Result<Vec<daw::manifest::Manifest>> = daw::repo::Repo::open(dir)
        .and_then(|repo| {
            // Newest first: list is chronological, so walk it backwards.
            repo.list_snapshots()?
                .iter()
                .rev()
                .map(|id| repo.read_snapshot(id))
                .collect()
        });

    match result {
        Ok(manifests) => {
            if manifests.is_empty() {
                println!("no snapshots");
            }
            for m in manifests {
                println!(
                    "{}  {}  {} files  {}",
                    m.id,
                    daw::manifest::fmt_timestamp(m.created_at),
                    m.files.len(),
                    m.message.as_deref().unwrap_or("")
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_restore(id: &str, force: bool, dir: &Path) -> ExitCode {
    match daw::repo::Repo::open(dir).and_then(|r| r.restore(id, force)) {
        Ok(plan) => {
            if plan.is_empty() {
                println!("already at {id}");
            } else {
                for change in &plan.changes {
                    println!("{change}");
                }
                println!("restored {id}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
