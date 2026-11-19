//! The Rust port of `openai_analytics.py`: read-only Codex account analytics through the
//! documented local app-server API (JSON-RPC over the stdio of a short-lived `codex app-server`).
//!
//! Errors are `"TypeName: message"`, as the bridge raises them. Rust callers use `native`.

#![forbid(unsafe_code)]

use crate::pynum::Num;
pub use native::{collect, collect_with, read_account_from};
use pycompat::{dumps, get, py_str, truthy};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const NOT_FOUND: &str =
    "RuntimeError: Codex app-server was not found. Open the Codex app or install the Codex CLI.";
const TIMED_OUT: &str = "RuntimeError: OpenAI account analytics timed out; try the next refresh.";
const CLOSED: &str = "RuntimeError: Codex app-server closed before returning account analytics.";
const FIELDS: [&str; 5] = [
    "lifetimeTokens",
    "peakDailyTokens",
    "longestRunningTurnSec",
    "currentStreakDays",
    "longestStreakDays",
];

/// Port of `codex_executable` (`openai_analytics.py:13`): `codex` on the PATH, else the newest
/// `codex.exe` the Codex app installed.
pub fn codex_executable() -> Result<Option<String>, String> {
    if let Ok(found) = which::which("codex") {
        return Ok(Some(found.to_string_lossy().into_owned()));
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let root = base.or_else(std::env::home_dir).unwrap_or_default();
    let newest = std::fs::read_dir(root.join("OpenAI").join("Codex").join("bin"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|dir| dir.path().join("codex.exe"))
        .filter_map(|exe| Some((exe.metadata().ok()?.modified().ok()?, exe)))
        .max_by_key(|(modified, _)| *modified);
    Ok(newest.map(|(_, exe)| exe.to_string_lossy().into_owned()))
}

/// Port of `read_account` (`openai_analytics.py:22`); `timeout` is in seconds.
pub fn read_account(timeout: i64) -> Result<serde_json::Value, String> {
    let executable = codex_executable()?.filter(|found| !found.is_empty());
    read_account_from(Path::new(&executable.ok_or(NOT_FOUND)?), timeout)
}

/// A count as Python sees it: an int or a float that is not negative; never a bool.
fn count(v: &Value) -> Option<Num> {
    let n = v.as_i64().map(Num::Int).or(v.as_f64().map(Num::Float));
    n.filter(|n| n.f() >= 0.0)
}

/// Whether `datetime.strptime(day, '%Y-%m-%d')` accepts `day`.
fn is_date(day: &str) -> bool {
    let digits = |s: &str, most| s.len() <= most && s.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<&str> = day.split('-').collect();
    let &[y, m, d] = parts.as_slice() else {
        return false;
    };
    let d = d.strip_prefix(' ').filter(|d| d.len() == 1).unwrap_or(d);
    let shaped = y.len() == 4 && y != "0000" && digits(y, 4) && digits(m, 2) && digits(d, 2);
    shaped && chrono::NaiveDate::parse_from_str(&format!("{y}-{m}-{d}"), "%Y-%m-%d").is_ok()
}

/// Port of `normalize` (`openai_analytics.py:81`): the five summary counts (`None` when absent
/// or not a count) and tokens per day, sorted by day; `daily` is `None` without a bucket list.
pub fn normalize(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let empty = json!({});
    let summary = get(&body, "summary")?.filter(|s| truthy(s));
    let summary = summary.unwrap_or(&empty);
    let mut clean = Map::new();
    for field in FIELDS {
        let value = get(summary, field)?.filter(|v| count(v).is_some());
        clean.insert(field.into(), value.cloned().unwrap_or_default());
    }
    let mut daily = Value::Null;
    if let Some(Value::Array(rows)) = get(&body, "dailyUsageBuckets")? {
        let mut days = BTreeMap::new();
        for row in rows {
            let (day, tokens) = (get(row, "startDate")?, get(row, "tokens")?);
            let day = day.and_then(Value::as_str).filter(|day| is_date(day));
            if let (Some(day), Some(tokens)) = (day, tokens.and_then(count)) {
                let total = days.entry(day).or_insert(Num::Int(0));
                *total = *total + tokens;
            }
        }
        let value = |n: Num| match n {
            Num::Int(i) => json!(i),
            Num::Float(x) => json!(x),
        };
        daily = Value::Object(
            days.iter()
                .map(|(d, n)| (d.to_string(), value(*n)))
                .collect(),
        );
    }
    Ok(json!({"summary": clean, "daily": daily}))
}

fn send(stdin: &mut ChildStdin, body: &Value) -> Result<(), String> {
    let mut line = dumps(body);
    line.push(b'\n');
    let sent = stdin.write_all(&line).and_then(|()| stdin.flush());
    sent.map_err(|e| format!("OSError: {e}"))
}

/// Sends one request and waits, until `deadline`, for the response with its id.
fn request(
    (stdin, rows, deadline): (&mut ChildStdin, &Receiver<Value>, Instant),
    id: i64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    send(
        stdin,
        &json!({"id": id, "method": method, "params": params}),
    )?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(TIMED_OUT.into());
        }
        let row = match rows.recv_timeout(remaining) {
            Ok(Value::Null) | Err(RecvTimeoutError::Disconnected) => return Err(CLOSED.into()),
            Err(RecvTimeoutError::Timeout) => return Err(TIMED_OUT.into()),
            Ok(row) => row,
        };
        let row_id = get(&row, "id")?;
        let row_id =
            row_id.and_then(|v| v.as_f64().or(v.as_bool().map(|b| f64::from(u8::from(b)))));
        if row_id != Some(id as f64) {
            continue;
        }
        if let Some(error) = row.get("error") {
            let code = if truthy(error) {
                get(error, "code")?
            } else {
                None
            };
            let code = code.map_or_else(|| "None".to_string(), py_str);
            return Err(format!(
                "RuntimeError: OpenAI analytics unavailable (RPC {code}). Check Codex login and version."
            ));
        }
        let result = get(&row, "result")?.filter(|r| truthy(r)).cloned();
        return Ok(result.unwrap_or_else(|| json!({})));
    }
}

/// For Rust callers: the bridge only sees the functions at the top of this file.
pub mod native {
    use super::*;

    /// `read_account` on a given executable: starts `<executable> app-server`, asks for the
    /// account usage report, and stops the process whatever the outcome.
    pub fn read_account_from(executable: &Path, timeout: i64) -> Result<Value, String> {
        let mut command = Command::new(executable);
        command.arg("app-server").stdin(Stdio::piped());
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)] // CREATE_NO_WINDOW: no console window flashes up
        std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
        let mut process = command.spawn().map_err(|e| format!("OSError: {e}"))?;
        let (sender, rows) = mpsc::channel();
        if let Some(stdout) = process.stdout.take() {
            // Ends, and so closes the channel, when the process closes its output.
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Ok(row) = serde_json::from_str::<Value>(&line) {
                        let _ = sender.send(row);
                    }
                }
            });
        }
        let seconds = Duration::from_secs(timeout.clamp(0, 1 << 30) as u64);
        let deadline = Instant::now() + seconds;
        let mut stdin = process.stdin.take();
        let mut exchange = || {
            let stdin = stdin.as_mut().ok_or("OSError: no pipe to the app-server")?;
            let client = json!({"clientInfo": {"name": "ai_usage_tracker", "version": "2.0"},
                                "capabilities": {"experimentalApi": true}});
            request((stdin, &rows, deadline), 1, "initialize", client)?;
            send(stdin, &json!({"method": "initialized"}))?;
            request(
                (stdin, &rows, deadline),
                2,
                "account/usage/read",
                Value::Null,
            )
        };
        let usage = exchange();
        let _ = process.kill();
        let _ = process.wait();
        usage
    }

    /// `collect` with the account reader given: the report for the tracker, never an error.
    pub fn collect_with(
        read: impl FnOnce() -> Result<Value, String>,
        log: &mut dyn FnMut(&str),
    ) -> Value {
        log("Fetching OpenAI account analytics ...");
        match read().and_then(normalize) {
            Ok(mut data) => {
                let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f+00:00");
                data["source"] = json!("live");
                data["as_of"] = json!(now.to_string());
                data["error"] = Value::Null;
                log("  ok");
                data
            }
            Err(error) => {
                log("  account analytics unavailable");
                let error = error.strip_prefix("RuntimeError: ");
                let error = error.unwrap_or("Could not read OpenAI account analytics.");
                json!({"summary": {}, "daily": null, "source": null, "as_of": null, "error": error})
            }
        }
    }

    /// Port of `collect` (`openai_analytics.py:102`): `{"summary", "daily", "source", "as_of",
    /// "error"}`; `log` gets the progress lines.
    pub fn collect(log: &mut dyn FnMut(&str)) -> Value {
        collect_with(|| read_account(25), log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_does_not_break_tracker() {
        let mut lines = Vec::new();
        let read = || Err("RuntimeError: Unavailable".to_string());
        let data = collect_with(read, &mut |line| lines.push(line.to_string()));
        let expected = json!({"summary": {}, "daily": null, "source": null, "as_of": null, "error": "Unavailable"});
        assert_eq!(data, expected);
        assert_eq!(lines.len(), 2);
        let data = collect_with(|| Err("OSError: gone".to_string()), &mut |_| ());
        assert_eq!(data["error"], "Could not read OpenAI account analytics.");
    }

    #[test]
    fn live_report_does_not_export_extra_fields() {
        let body = json!({"summary": {"lifetimeTokens": 123, "email": "private"}, "other": "private",
                          "dailyUsageBuckets": [{"startDate": "2026-10-31", "tokens": 1.5},
                                                {"startDate": "2026-02-30", "tokens": 1},
                                                {"startDate": "2026-10-31", "tokens": true}]});
        let data = collect_with(|| Ok(body), &mut |_| ());
        assert_eq!(data["summary"]["lifetimeTokens"], 123);
        assert_eq!(data["daily"], json!({"2026-10-31": 1.5}));
        assert!(!data.to_string().contains("private"));
        assert_eq!(
            (&data["source"], &data["error"]),
            (&json!("live"), &Value::Null)
        );
        let as_of = data["as_of"].as_str().unwrap_or_default();
        assert!(pycompat::iso_micros(as_of).is_some() && as_of.len() == 32);
    }

    /// Drives `read_account_from` against fake app-servers: shell scripts that log what they
    /// were sent next to themselves.
    #[cfg(unix)]
    #[test]
    fn talks_to_the_app_server() {
        use std::os::unix::fs::PermissionsExt;
        const REPLY: &str = "read -r a; echo \"$1\" > \"$0.log\"; echo \"$a\" >> \"$0.log\"; echo junk; echo '{\"id\": 1, \"result\": {}}'
read -r b; read -r c; echo \"$b\" >> \"$0.log\"; echo \"$c\" >> \"$0.log\"
echo '{\"method\": \"note\"}'; echo '{\"id\": 2, \"result\": {\"summary\": {\"lifetimeTokens\": 5}}}'; exec sleep 30";
        const SENT: &str = r#"app-server
{"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "ai_usage_tracker", "version": "2.0"}, "capabilities": {"experimentalApi": true}}}
{"method": "initialized"}
{"id": 2, "method": "account/usage/read", "params": null}
"#;
        let rpc = "RuntimeError: OpenAI analytics unavailable (RPC -32600). Check Codex login and version.";
        let cases = [
            (REPLY, Ok(json!({"summary": {"lifetimeTokens": 5}}))),
            ("exec sleep 30", Err(TIMED_OUT.to_string())),
            ("read -r a", Err(CLOSED.to_string())),
            (
                "read -r a; echo '{\"id\": 1.0, \"error\": {\"code\": -32600}}'; exec sleep 30",
                Err(rpc.to_string()),
            ),
        ];
        let dir = std::env::temp_dir().join(format!("openai-analytics-rs-{}", std::process::id()));
        let scripts: Vec<PathBuf> = (0..cases.len())
            .map(|i| dir.join(format!("codex{i}")))
            .collect();
        let made = std::fs::create_dir_all(&dir);
        for (script, (body, _)) in scripts.iter().zip(&cases) {
            let written = std::fs::write(script, format!("#!/bin/sh\n{body}\n"));
            let mode = std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755));
            assert!(made.is_ok() && written.is_ok() && mode.is_ok());
        }
        let started = Instant::now();
        for (script, (_, expected)) in scripts.iter().zip(cases) {
            assert_eq!(read_account_from(script, 1), expected);
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        let log = std::fs::read_to_string(dir.join("codex0.log")).map_err(|e| e.to_string());
        assert_eq!(log, Ok(SENT.to_string()));
        let missing = read_account_from(&dir.join("missing"), 1);
        assert!(missing.is_err_and(|e| e.starts_with("OSError: ")));
        let _ = std::fs::remove_dir_all(dir);
    }
}
