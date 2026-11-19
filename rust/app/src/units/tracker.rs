//! The `tracker` command: scan the logs, check the limits, write the dashboard. It reads its
//! flags the way the original's argparse did and prints what the original printed.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use api_billing_rs::port::native as billing;
use clap::ArgMatches;
use serde_json::json;
use tracker_rs::port::native::{Dirs, write_dashboard};

const USAGE: &str = "usage: tracker [-h] [--out OUT] [--no-open] [--set-balance PROVIDER USD]";
const HELP: &str = "
AI usage tracker: scans local Claude Code and Codex logs, checks live plan
limits, writes an HTML dashboard.

options:
  -h, --help            show this help message and exit
  --out OUT
  --no-open             don't open the dashboard in the browser
  --set-balance PROVIDER USD
                        record your current API credit balance, e.g. --set-
                        balance anthropic 25.00 (use 'clear' to remove)";
const FLAGS: [&str; 4] = ["--help", "--out", "--no-open", "--set-balance"];

#[derive(Debug, Default, PartialEq)]
struct Args {
    help: bool,
    out: Option<String>,
    no_open: bool,
    set_balance: Option<(String, String)>,
}

/// argparse's reading of the command line: long flags may be shortened to any prefix only one
/// flag starts with, `--out=FILE` is `--out FILE`, and a value never starts with `-`.
fn parse(argv: &[String]) -> Result<Args, String> {
    let (mut args, mut extra, mut i) = (Args::default(), Vec::new(), 0);
    let is_value = |s: &&String| !s.starts_with('-') || s.len() == 1;
    while let Some(arg) = argv.get(i) {
        i += 1;
        let (name, attached) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(n, v)| (n, Some(v)));
        let matching: Vec<&str> = match name {
            "-h" => vec!["--help"],
            _ if name.starts_with("--") && name.len() > 2 => {
                let exact = FLAGS.iter().find(|f| **f == name);
                exact.map_or_else(
                    || {
                        FLAGS
                            .iter()
                            .filter(|f| f.starts_with(name))
                            .copied()
                            .collect()
                    },
                    |f| vec![*f],
                )
            }
            _ => Vec::new(),
        };
        let flag = match matching[..] {
            [flag] => flag,
            [] => {
                extra.push(arg.clone());
                continue;
            }
            _ => {
                let all: Vec<String> = matching.iter().map(ToString::to_string).collect();
                return Err(format!(
                    "ambiguous option: {name} could match {}",
                    all.join(", ")
                ));
            }
        };
        let mut values: Vec<String> = attached.map(ToString::to_string).into_iter().collect();
        let wanted = match flag {
            "--out" => 1,
            "--set-balance" => 2,
            _ => 0,
        };
        if wanted == 0 && attached.is_some() {
            return Err(format!(
                "argument {}: ignored explicit argument '{}'",
                display(flag),
                values[0]
            ));
        }
        while values.len() < wanted && argv.get(i).is_some_and(|s| is_value(&s)) {
            values.push(argv[i].clone());
            i += 1;
        }
        if values.len() < wanted {
            let count = if wanted == 1 {
                "one argument".to_string()
            } else {
                format!("{wanted} arguments")
            };
            return Err(format!("argument {flag}: expected {count}"));
        }
        match flag {
            "--help" => args.help = true,
            "--no-open" => args.no_open = true,
            "--out" => args.out = values.pop(),
            _ => args.set_balance = values.pop().and_then(|usd| Some((values.pop()?, usd))),
        }
        if args.help {
            return Ok(args);
        }
    }
    if extra.is_empty() {
        Ok(args)
    } else {
        Err(format!("unrecognized arguments: {}", extra.join(" ")))
    }
}

fn display(flag: &str) -> &str {
    if flag == "--help" { "-h/--help" } else { flag }
}

/// Python's `f"{amount:,.2f}"`.
fn with_commas(amount: f64) -> String {
    let fixed = format!("{:.2}", amount.abs());
    let (whole, cents) = fixed.split_once('.').unwrap_or((&fixed, "00"));
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let sign = if amount.is_sign_negative() && !amount.is_nan() {
        "-"
    } else {
        ""
    };
    if whole.bytes().all(|b| b.is_ascii_digit()) {
        format!("{sign}{grouped}.{cents}")
    } else {
        format!("{amount}")
    }
}

/// `Path(out).resolve()`: absolute, with the links of the folder followed when it exists.
fn resolve(out: &str) -> PathBuf {
    let path = std::path::absolute(out).unwrap_or_else(|_| PathBuf::from(out));
    let folder = path.parent().and_then(|p| p.canonicalize().ok());
    match (folder, path.file_name()) {
        (Some(folder), Some(name)) => folder.join(name),
        _ => path,
    }
}

fn set_balance(provider: &str, usd: &str) -> Result<(), (u8, String)> {
    let names = billing::PROVIDERS.map(|(name, _, _)| name);
    let Some(name) = names.iter().find(|n| n.eq_ignore_ascii_case(provider)) else {
        let message = format!("provider must be one of: {}", names.join(", "));
        return Err((2, format!("{USAGE}\ntracker: error: {message}")));
    };
    let amount = if usd.eq_ignore_ascii_case("clear") {
        None
    } else {
        Some(pycompat::to_float(&json!(usd.trim_start_matches('$'))).map_err(|e| (1, e))?)
    };
    billing::set_balance(name, amount, None);
    match amount {
        Some(amount) => println!("{name} balance set to ${}", with_commas(amount)),
        None => println!("{name} balance cleared"),
    }
    Ok(())
}

fn build(dirs: &Dirs, out: &Path) -> Result<(), String> {
    let mut log = |line: &str| println!("{line}");
    let data = dirs.collect(&mut log, &mut |log| billing::api_usage(log), &mut |log| {
        openai_analytics_rs::port::native::collect(log)
    })?;
    write_dashboard(&data, out).map_err(|e| format!("OSError: {e}"))?;
    println!("Wrote {}", out.display());
    Ok(())
}

pub fn run(_command: &str, matches: &ArgMatches) -> ExitCode {
    let typed = matches.get_many::<OsString>("args").into_iter().flatten();
    let argv: Vec<String> = typed.map(|a| a.to_string_lossy().into_owned()).collect();
    let args = match parse(&argv) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{USAGE}\ntracker: error: {message}");
            return ExitCode::from(2);
        }
    };
    if args.help {
        println!("{USAGE}\n{HELP}");
        return ExitCode::SUCCESS;
    }
    if let Some((provider, usd)) = &args.set_balance
        && let Err((code, message)) = set_balance(provider, usd)
    {
        eprintln!("{message}");
        return ExitCode::from(code);
    }
    let mut dirs = Dirs::from_env();
    // pricing.json and the default dashboard.html sit beside the program; AI_USAGE_TRACKER_DIR
    // names another folder (the recorded cases point it at the original's).
    if let Some(folder) = std::env::var_os("AI_USAGE_TRACKER_DIR") {
        dirs.here = folder.into();
    }
    let out = args
        .out
        .as_deref()
        .map_or_else(|| dirs.here.join("dashboard.html"), resolve);
    if let Err(message) = build(&dirs, &out) {
        eprintln!("{message}");
        return ExitCode::FAILURE;
    }
    if !args.no_open
        && let Err(e) = open::that_detached(&out)
    {
        eprintln!("Could not open {}: {e}", out.display());
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> Result<Args, String> {
        parse(
            &line
                .split_whitespace()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn reads_flags_as_argparse_does() {
        let both = Args {
            out: Some("a.html".into()),
            no_open: true,
            ..Args::default()
        };
        assert_eq!(parsed("--out a.html --no-open"), Ok(both));
        let short = Args {
            out: Some("a.html".into()),
            no_open: true,
            ..Args::default()
        };
        assert_eq!(parsed("--no --o=a.html"), Ok(short));
        let balance = Args {
            set_balance: Some(("anthropic".into(), "25".into())),
            ..Args::default()
        };
        assert_eq!(parsed("--set anthropic 25"), Ok(balance));
        assert_eq!(parsed("-h extra").map(|a| a.help), Ok(true));
        let expect = |line: &str, message: &str| assert_eq!(parsed(line), Err(message.to_string()));
        expect("extra --no-open more", "unrecognized arguments: extra more");
        expect("--frobnicate", "unrecognized arguments: --frobnicate");
        expect(
            "--set-balance anthropic",
            "argument --set-balance: expected 2 arguments",
        );
        expect(
            "--set-balance anthropic --no-open",
            "argument --set-balance: expected 2 arguments",
        );
        expect("--out", "argument --out: expected one argument");
        expect(
            "--no-open=1",
            "argument --no-open: ignored explicit argument '1'",
        );
    }

    #[test]
    fn groups_thousands() {
        let shown: Vec<String> = [25.0, 1234.5, 1_234_567.891, 0.005, -1234.0, 999.999]
            .map(with_commas)
            .into();
        assert_eq!(
            shown,
            [
                "25.00",
                "1,234.50",
                "1,234,567.89",
                "0.01",
                "-1,234.00",
                "1,000.00"
            ]
        );
    }
}
