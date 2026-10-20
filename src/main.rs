use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Temporary manual-test entry point: `daw <project-dir>` prints the
    // SHA-256 and path of every scanned file. Real subcommands
    // (init/status/...) arrive with clap in a later milestone.
    let Some(dir) = env::args().nth(1) else {
        eprintln!("usage: daw <project-dir>");
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
