//! Runs `estimate_balance` on `{"entry", "daily"}` lines from stdin and prints each result.
//! `scripts/compare_balance.py` runs the Python original on the same lines and compares.

use api_billing_rs::port::estimate_balance;
use serde_json::{Value, json};
use std::io::BufRead;

fn main() {
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let case: Value = serde_json::from_str(&line).unwrap_or_default();
        let out = match estimate_balance(case["entry"].clone(), case["daily"].clone()) {
            Ok(value) => json!({"ok": value}),
            Err(error) => json!({"error": error}),
        };
        println!("{out}");
    }
}
