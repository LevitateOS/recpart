use clap::Parser;
use recpart::cli::{run, to_json, Cli};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if json {
                eprintln!("{}", to_json(&err.payload()));
            } else {
                eprintln!("recpart: {} [{}]\n  Expected: {}\n  Found: {}\n  Next: {}", err.code, err.component, err.expectation, err.observed, err.remediation);
            }
            ExitCode::from(err.code.exit_status())
        }
    }
}
