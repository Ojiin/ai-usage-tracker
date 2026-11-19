use std::ffi::OsString;

use clap::{Arg, Command, value_parser};

/// The command `units::dispatch` gets when argv names no subcommand.
pub const ROOT: &str = "tracker";

/// Every argument goes to the glue as it was typed: `units/tracker.rs` reads the flags the
/// way the original's argparse did, so usage errors come out the same.
pub fn command() -> Command {
    let args = Arg::new("args")
        .num_args(0..)
        .trailing_var_arg(true)
        .allow_hyphen_values(true)
        .value_parser(value_parser!(OsString));
    Command::new(ROOT)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(args)
}
