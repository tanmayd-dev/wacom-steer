use clap::Parser;
use std::process::ExitCode;
use wacom_steer::config::CliArgs;

fn main() -> ExitCode {
    let args = CliArgs::parse();
    if let Err(err) = wacom_steer::launch::run(args) {
        eprintln!("Error: {err}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
