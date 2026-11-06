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
                eprintln!("recpart: {}", err);
            }
            ExitCode::from(err.code.exit_status())
        }
    }
}
