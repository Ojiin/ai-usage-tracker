//! Placeholder so `rust-anything app lift` takes this crate along; removed after the lift.

use std::process::ExitCode;

use clap::ArgMatches;

pub fn run(_command: &str, _matches: &ArgMatches) -> ExitCode {
    ExitCode::from(2)
}
