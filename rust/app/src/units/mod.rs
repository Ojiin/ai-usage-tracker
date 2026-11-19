//! Written by `rust-anything app lift`: every command is answered in Rust.

use std::process::ExitCode;

use clap::ArgMatches;

mod tracker;

pub fn dispatch(command: &str, matches: &ArgMatches) -> Option<ExitCode> {
    match command {
        "tracker" => Some(tracker::run(command, matches)),
        _ => None,
    }
}
