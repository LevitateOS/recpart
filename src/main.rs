use clap::Parser;
use distro_spec::shared::error::ToolErrorCode;
use recpart::cli::{run, Cli};
use recpart::json::to_pretty_json;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json_requested = cli.json_requested();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if json_requested {
                match to_pretty_json(&err.payload()) {
                    Ok(json) => eprintln!("{}", json),
                    Err(_) => eprintln!("recpart: {}", err),
                }
            } else {
                eprintln!("recpart: {}", err);
            }
            ExitCode::from(err.code.exit_code())
        }
    }
}
