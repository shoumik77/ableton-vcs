use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Temporary manual-test entry point: `daw <project-dir>` prints the files
    // the scanner finds. Real subcommands (init/status/...) arrive with clap
    // in a later milestone.
    let Some(dir) = env::args().nth(1) else {
        eprintln!("usage: daw <project-dir>");
        return ExitCode::FAILURE;
    };

    match daw::scanner::scan_project(Path::new(&dir)) {
        Ok(files) => {
            for file in files {
                println!("{}", file.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
