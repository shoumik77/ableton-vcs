use std::env;
use std::path::Path;
use std::process::ExitCode;

// Hand-rolled subcommand dispatch for now — clap arrives in a later
// milestone once the command surface stabilises.
fn main() -> ExitCode {
    let Some(command) = env::args().nth(1) else {
        usage();
        return ExitCode::FAILURE;
    };

    match command.as_str() {
        "init" => cmd_init(env::args().nth(2)),
        "snapshot" => cmd_snapshot(env::args().skip(2)),
        "hash" => cmd_hash(env::args().nth(2)),
        "status" => cmd_status(env::args().nth(2)),
        "diff" => cmd_diff(env::args().skip(2)),
        "log" => cmd_log(env::args().nth(2)),
        _ => {
            eprintln!("unknown command: {command}");
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!("usage:");
    eprintln!("  daw init [dir]                 start tracking a project directory");
    eprintln!("  daw snapshot [dir] [-m msg]    record a manifest of the project");
    eprintln!("  daw hash <dir>                 debug: print hash + path per scanned file");
    eprintln!(
        "  daw status [dir]               show added/modified/deleted files vs latest snapshot"
    );
    eprintln!("  daw diff <a> [b] [--dir dir]   compare two snapshots (or a vs working tree)");
    eprintln!("  daw log [dir]                  list snapshots, newest first");
}

fn cmd_init(dir: Option<String>) -> ExitCode {
    let dir = dir.unwrap_or_else(|| ".".into());
    match daw::repo::Repo::init(Path::new(&dir)) {
        Ok(_) => {
            println!(
                "initialised {}",
                Path::new(&dir).join(daw::repo::DIR).display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_snapshot(mut args: impl Iterator<Item = String>) -> ExitCode {
    // `daw snapshot [dir] [-m message]`: first bare argument is the project
    // dir, `-m`/`--message` consumes the next argument.
    let mut dir = ".".to_string();
    let mut message = None;
    while let Some(arg) = args.next() {
        if arg == "-m" || arg == "--message" {
            match args.next() {
                Some(m) => message = Some(m),
                None => {
                    eprintln!("error: {arg} requires a value");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            dir = arg;
        }
    }

    match daw::repo::Repo::open(Path::new(&dir)).and_then(|r| r.create_snapshot(message)) {
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

fn cmd_hash(dir: Option<String>) -> ExitCode {
    let Some(dir) = dir else {
        eprintln!("usage: daw hash <project-dir>");
        return ExitCode::FAILURE;
    };

    match daw::hash::hash_project(Path::new(&dir)) {
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

fn cmd_status(dir: Option<String>) -> ExitCode {
    let dir = dir.unwrap_or_else(|| ".".into());
    match daw::repo::Repo::open(Path::new(&dir)).and_then(|r| r.status()) {
        Ok(diff) => print_diff(&diff, ExitCode::from(1)),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_diff(mut args: impl Iterator<Item = String>) -> ExitCode {
    // `daw diff <a> [b] [--dir path]`: bare arguments are snapshot ids;
    // omitting `b` compares `a` to the working tree.
    let mut ids = Vec::new();
    let mut dir = ".".to_string();
    while let Some(arg) = args.next() {
        if arg == "--dir" {
            match args.next() {
                Some(d) => dir = d,
                None => {
                    eprintln!("error: --dir requires a value");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            ids.push(arg);
        }
    }

    let Some(a) = ids.first() else {
        eprintln!("usage: daw diff <a> [b] [--dir dir]");
        return ExitCode::FAILURE;
    };

    let result = daw::repo::Repo::open(Path::new(&dir)).and_then(|repo| match ids.get(1) {
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

fn cmd_log(dir: Option<String>) -> ExitCode {
    let dir = dir.unwrap_or_else(|| ".".into());
    let result: std::io::Result<Vec<daw::manifest::Manifest>> =
        daw::repo::Repo::open(Path::new(&dir)).and_then(|repo| {
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
