//! Runs `due_alerts` on scenarios from stdin, one JSON object per line
//! (`{"entries", "now", "path"}`), and prints the alerts and the state file after each.
//! `scripts/compare_alerts.py` runs the Python original on the same lines and compares.

use limits_watch_rs::port::native::due_alerts;
use serde_json::{Value, json};
use std::io::BufRead;
use std::path::Path;

fn main() {
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(case) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let path = Path::new(case["path"].as_str().unwrap_or_default());
        let now = case["now"].as_f64().unwrap_or_default();
        let alerts = due_alerts(&case["entries"], now, path, &|t| format!("T{t}"));
        let state: Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        println!("{}", json!({"alerts": alerts, "state": state}));
    }
}
