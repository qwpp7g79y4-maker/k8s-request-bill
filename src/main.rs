use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;
use k8s_request_bill::cli::{execute, Cli};

mod stdout_probe;

fn main() -> ExitCode {
    if stdout_probe::stdout_was_closed_at_start() {
        eprintln!("error: stdout is closed");
        return ExitCode::from(2);
    }
    let cli = Cli::parse();
    match execute(&cli, &mut io::stdin().lock()) {
        Ok(text) => {
            // A closed pipe (`| head`) is not an error worth reporting.
            match io::stdout().lock().write_all(text.as_bytes()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: cannot write output: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
