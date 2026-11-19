//! The Rust half of `scripts/compare_format.py`.
//!
//! Reads one JSON case per line on stdin and prints one JSON answer per line: the text the
//! widget's Rust code produces for it. The script computes the same with the functions in
//! widget.py and compares. Times use the zone in `TZ`, as the Python side does.

use ai_usage_tracker_lib::format::{self, Num, Zone};
use ai_usage_tracker_lib::view::{self, Clock, View};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// A number sent exactly: `{"int": "123"}` or `{"bits": "4638387860618067575"}` (an f64).
fn num(case: &Value) -> Option<Num> {
    let int = case.get("int").and_then(Value::as_str);
    let bits = case.get("bits").and_then(Value::as_str);
    match (int, bits) {
        (Some(int), _) => int.parse().ok().map(Num::Int),
        (_, Some(bits)) => bits.parse().ok().map(|b| Num::Float(f64::from_bits(b))),
        _ => None,
    }
}

fn answer(case: &Value) -> Option<Value> {
    let text = |key: &str| case.get(key).and_then(Value::as_str);
    Some(match text("op")? {
        "fmt" => json!(format::fmt(num(case)?)),
        "until" => json!(format::until(num(case)?.f())),
        "money" => json!(format::money(num(case)?.f())),
        "percent" => json!(format::percent(num(case)?.f())),
        "thousands" => json!(format::thousands(num(case)?)),
        "clip" => json!(view::scan_failed(text("text")?)),
        "amount" => json!(format::parse_amount(text("text")?)),
        "strftime" => json!(Zone::Local.strftime(num(case)?.f(), text("pattern")?)),
        "iso" => {
            let epoch = Zone::Local.parse_iso(text("text")?);
            json!(epoch.and_then(|epoch| Zone::Local.strftime(epoch, text("pattern")?)))
        }
        "working" => json!(view::working(case.get("active")?)),
        "view" => {
            let clock = Clock {
                now: case.get("now")?.as_f64()?,
                zone: Zone::Local,
            };
            let data = case.get("data")?;
            json!({
                "labels": View::build(data, &clock).labels(),
                "status": view::updated(data, &clock).text,
            })
        }
        _ => return None,
    })
}

fn main() {
    let (stdin, stdout) = (std::io::stdin(), std::io::stdout());
    let mut out = std::io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines().map_while(Result::ok) {
        let case: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let reply = answer(&case).unwrap_or_else(|| json!({"error": "bad case"}));
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}
