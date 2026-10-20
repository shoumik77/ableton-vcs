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
