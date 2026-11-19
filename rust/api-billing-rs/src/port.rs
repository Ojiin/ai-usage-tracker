//! The Rust port of `api_billing.py`: API spend, token usage and an estimated remaining
//! balance for Anthropic and OpenAI, from each provider's admin reports.
//!
//! The functions at the top are what the bridge calls, with Python's errors for wrong shapes.
//! Rust callers use `native`: typed arguments, and the HTTP call passed in as a closure so
//! paging and report parsing run on canned pages in the tests. Errors are `"TypeName: message"`.
//! `get_json` and the two report functions raise the module's own `ApiError`, and `utc_day`,
//! `summarize`, `load_json` and `save_json` take a datetime or a `Path`: none of that crosses
//! the bridge, so those stay in Python there and are ported in `native` only.

#![forbid(unsafe_code)]

use crate::pynum::Num;
use chrono::{DateTime, Datelike, Duration, Utc};
pub use native::{PROVIDERS, api_usage, api_usage_at, paged, set_balance_in, store};
use pycompat::{get, iso_micros, item, no_attr, py_repr, quote_plus, read_json, round_to};
use pycompat::{str_of, to_float, truthy, ty, write_json_indented};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const FIELDS: [&str; 5] = ["input", "cache_write", "cache_read", "output", "requests"];
const DAYS: i64 = 31; // history shown in the dashboard; both reports allow 31 daily buckets
const STAMP: &str = "%Y-%m-%dT%H:%M:%SZ";
const DAY_START: &str = "%Y-%m-%dT00:00:00Z";

fn plain_dict(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object().filter(|m| !m.contains_key("$num"))
}

fn num(v: &Value) -> Result<Num, String> {
    let int = v.as_i64().map(Num::Int);
    int.map_or_else(|| to_float(v).map(Num::Float), Ok)
}

/// Python's `round(n, places)`: an int stays an int.
fn rounded(n: Num, places: usize) -> Value {
    match n {
        Num::Int(i) => json!(i),
        Num::Float(x) => json!(round_to(x, places)),
    }
}

/// Port of `admin_key` (`api_billing.py:27`).
pub fn admin_key(env: serde_json::Value) -> Result<Option<String>, String> {
    match &env {
        Value::String(name) => Ok(native::admin_key(name)),
        other => Err(format!("TypeError: str expected, not {}", ty(other))),
    }
}

/// Port of `bucket_day` (`api_billing.py:87`): the UTC day of a bucket; Anthropic uses RFC 3339
/// strings, OpenAI unix seconds.
pub fn bucket_day(bucket: serde_json::Value) -> Result<Option<String>, String> {
    let start = get(&bucket, "starting_at")?.filter(|s| truthy(s));
    let seconds = match start.or(get(&bucket, "start_time")?) {
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::MAX),
        Some(other) if truthy(other) => return Ok(Some(str_of(other).chars().take(10).collect())),
        _ => return Ok(None),
    };
    let at = DateTime::from_timestamp(seconds.floor() as i64, 0);
    let day = at
        .filter(|t| (1..=9999).contains(&t.year()))
        .map(native::utc_day);
    day.map(Some)
        .ok_or_else(|| "ValueError: year is out of range".to_string())
}

/// Port of `new_tokens` (`api_billing.py:95`).
pub fn new_tokens() -> Result<serde_json::Value, String> {
    Ok(FIELDS.iter().map(|f| (f.to_string(), json!(0))).collect())
}

/// Port of `set_balance` (`api_billing.py:153`): records the balance the user read off the
/// provider's billing page, or forgets it when `amount` is `None`.
pub fn set_balance(
    provider: serde_json::Value,
    amount: Option<f64>,
    today_spend: Option<serde_json::Value>,
) -> Result<(), String> {
    let hashable = || match plain_dict(&provider).is_some() || provider.is_array() {
        true => Err(format!("TypeError: unhashable type: '{}'", ty(&provider))),
        false => Ok(()),
    };
    let mut baseline = None;
    if amount.is_some() {
        match today_spend.filter(|t| !t.is_null()) {
            None => hashable()?,
            Some(spend) if truthy(&spend) => baseline = Some(to_float(&spend)?),
            Some(_) => baseline = Some(0.0),
        }
    }
    hashable()?;
    // A key that is not text is saved as `json.dumps` writes it: `true`, `null`, `0`.
    let name = provider
        .as_str()
        .map_or_else(|| provider.to_string(), str::to_string);
    set_balance_in(&store(), Utc::now(), &name, amount, baseline);
    Ok(())
}

/// Port of `estimate_balance` (`api_billing.py:170`): the entered amount minus the spend
/// reported since, without the part of that day's spend the amount already reflected.
pub fn estimate_balance(
    entry: serde_json::Value,
    daily: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !truthy(&entry) {
        return Ok(Value::Null);
    }
    let as_of = item(&entry, "as_of")?;
    let since_day: String = str_of(as_of).chars().take(10).collect();
    let mut spent = Num::Int(0);
    for (day, cost) in plain_dict(&daily).ok_or_else(|| no_attr(&daily, "items"))? {
        if !day.is_empty() && *day >= since_day {
            spent = spent + num(cost)?;
        }
    }
    if let Some(baseline) = entry.get("day_baseline").filter(|b| truthy(b)) {
        spent = spent - num(baseline)?;
    }
    let spent = Num::max(&[spent, Num::Float(0.0)]);
    let (amount, left) = (
        item(&entry, "amount")?,
        num(item(&entry, "amount")?)? - spent,
    );
    Ok(
        json!({"entered": amount, "as_of": as_of, "spent_since": rounded(spent, 2),
              "remaining": rounded(left, 2)}),
    )
}

/// For Rust callers: the bridge only sees the functions at the top of this file.
pub mod native {
    use super::*;

    /// Request headers, and the call that fetches one page: `(url, headers, provider)`.
    pub type Headers<'a> = [(&'a str, String)];
    pub type Fetch<'a> = dyn FnMut(&str, &Headers, &str) -> Result<Value, String> + 'a;
    /// Dollars per UTC day, tokens per UTC day, tokens per model.
    pub type Reports = [Map<String, Value>; 3];

    /// Provider, the variable holding its admin key, and where to create that key.
    pub const PROVIDERS: [(&str, &str, &str); 2] = [
        (
            "Anthropic",
            "ANTHROPIC_ADMIN_KEY",
            "https://platform.claude.com/settings/admin-keys",
        ),
        (
            "OpenAI",
            "OPENAI_ADMIN_KEY",
            "https://platform.openai.com/settings/organization/admin-keys",
        ),
    ];

    /// `%LOCALAPPDATA%\AIUsageTracker`, or the home folder where that is not set.
    pub fn store() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        base.or_else(std::env::home_dir)
            .unwrap_or_default()
            .join("AIUsageTracker")
    }

    #[cfg(windows)]
    fn saved_key(env: &str) -> Option<String> {
        let user = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let value: String = user.open_subkey("Environment").ok()?.get_value(env).ok()?;
        Some(value.trim().to_string()).filter(|key| !key.is_empty())
    }

    #[cfg(not(windows))]
    fn saved_key(_env: &str) -> Option<String> {
        None
    }

    /// The environment first; then the user's saved variables, so a running widget sees a key
    /// set after it started.
    pub fn admin_key(env: &str) -> Option<String> {
        let key = std::env::var(env).ok().filter(|key| !key.is_empty());
        key.map(|key| key.trim().to_string())
            .or_else(|| saved_key(env))
    }

    pub fn utc_day<Tz: chrono::TimeZone>(dt: DateTime<Tz>) -> String {
        dt.to_utc().format("%Y-%m-%d").to_string()
    }

    /// The JSON in the file, or `{}` when it cannot be read.
    pub fn load_json(path: &Path) -> Value {
        read_json(path).unwrap_or_else(|| json!({}))
    }

    /// Writes with one space of indent, as the original; a file that cannot be written is not
    /// an error.
    pub fn save_json(path: &Path, data: &Value) {
        let _ = write_json_indented(path, data);
    }

    /// One GET of a report page, with the original's messages for a refused key, another HTTP
    /// status and anything else (no network, not JSON).
    pub fn get_json(url: &str, headers: &Headers, provider: &str) -> Result<Value, String> {
        let timeout = Some(std::time::Duration::from_secs(20));
        let config = ureq::Agent::config_builder()
            .timeout_global(timeout)
            .build();
        let mut request = ureq::Agent::from(config).get(url);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let failed = match request
            .call()
            .map(|mut response| response.body_mut().read_json())
        {
            Ok(Ok(json)) => return Ok(json),
            Err(ureq::Error::StatusCode(code @ (401 | 403))) => format!(
                "rejected the admin key (HTTP {code}); it needs an admin key, not a normal API key"
            ),
            Err(ureq::Error::StatusCode(code)) => format!("report failed (HTTP {code})"),
            _ => "report failed (offline?)".to_string(),
        };
        Err(format!("ApiError: {provider} {failed}"))
    }

    /// Every bucket of a paginated report (both providers use data / has_more / next_page).
    pub fn paged(
        base_url: &str,
        params: &Headers,
        headers: &Headers,
        provider: &str,
        fetch: &mut Fetch,
    ) -> Result<Vec<Value>, String> {
        let (mut params, mut buckets) = (params.to_vec(), Vec::new());
        // Hard stop: 50 pages of 31 days is over four years.
        for _ in 0..50 {
            let pair = |(name, value): &(&str, String)| {
                format!("{}={}", quote_plus(name), quote_plus(value))
            };
            let query: Vec<String> = params.iter().map(pair).collect();
            let body = fetch(
                &format!("{base_url}?{}", query.join("&")),
                headers,
                provider,
            )?;
            buckets.extend(body["data"].as_array().into_iter().flatten().cloned());
            let next = body.get("next_page").filter(|next| truthy(next));
            let (true, Some(next)) = (truthy(&body["has_more"]), next) else {
                return Ok(buckets);
            };
            params.retain(|(name, _)| *name != "page");
            params.push(("page", str_of(next)));
        }
        Err(format!("ApiError: {provider} report had too many pages"))
    }

    fn int(row: &Value, field: &str) -> i64 {
        row[field].as_i64().unwrap_or(0)
    }

    /// What the two providers share. `names` are the parameters for the start and for grouping
    /// by model; `per_dollar` is 100 where amounts are cents; `row` reads the five `FIELDS`.
    #[allow(clippy::too_many_arguments)]
    fn reports(
        provider: &str,
        headers: &Headers,
        urls: [&str; 2],
        names: [&str; 2],
        starts: [String; 2],
        per_dollar: f64,
        row: fn(&Value) -> [i64; 5],
        fetch: &mut Fetch,
    ) -> Result<Reports, String> {
        let [cost, mut usage] = starts.map(|start| {
            vec![
                (names[0], start),
                ("bucket_width", "1d".into()),
                ("limit", "31".into()),
            ]
        });
        usage.push((names[1], "model".into()));
        let results = |b: &Value| b["results"].as_array().cloned().unwrap_or_default();
        let day = |b: &Value| bucket_day(b.clone()).map(Option::unwrap_or_default);
        let [mut daily, mut tokens, mut models] = [Map::new(), Map::new(), Map::new()];
        for bucket in paged(urls[0], &cost, headers, provider, fetch)? {
            let mut total = 0.0;
            for result in results(&bucket) {
                let amount = &result["amount"];
                let amount =
                    plain_dict(amount).map_or(amount, |a| a.get("value").unwrap_or(&Value::Null));
                total += if truthy(amount) {
                    to_float(amount)?
                } else {
                    0.0
                };
            }
            let spent = daily.entry(day(&bucket)?).or_insert(json!(0.0));
            *spent = json!(spent.as_f64().unwrap_or(0.0) + total / per_dollar);
        }
        for bucket in paged(urls[1], &usage, headers, provider, fetch)? {
            let day = day(&bucket)?;
            tokens.entry(day.clone()).or_insert(new_tokens()?);
            for result in results(&bucket) {
                let model = Some(&result["model"]).filter(|m| truthy(m));
                let model = model.map_or_else(|| "unknown".to_string(), str_of);
                models.entry(model.clone()).or_insert(new_tokens()?);
                for target in [&mut tokens[&day], &mut models[&model]] {
                    for (field, count) in FIELDS.iter().zip(row(&result)) {
                        target[*field] = json!(int(target, field) + count);
                    }
                }
            }
        }
        Ok([daily, tokens, models])
    }

    /// Anthropic's cost report (amounts are decimal strings in cents) and messages usage report
    /// by model, from the given RFC 3339 starts.
    pub fn anthropic_reports(
        key: &str,
        cost_start: &str,
        usage_start: &str,
        fetch: &mut Fetch,
    ) -> Result<Reports, String> {
        let agent = ("User-Agent", "AIUsageTracker/1.0".to_string());
        let headers = [
            ("x-api-key", key.into()),
            ("anthropic-version", "2023-06-01".into()),
            agent,
        ];
        let base = "https://api.anthropic.com/v1/organizations";
        let urls = [
            format!("{base}/cost_report"),
            format!("{base}/usage_report/messages"),
        ];
        let row = |r: &Value| {
            let cc = &r["cache_creation"];
            let written =
                int(cc, "ephemeral_1h_input_tokens") + int(cc, "ephemeral_5m_input_tokens");
            let read = int(r, "cache_read_input_tokens");
            [
                int(r, "uncached_input_tokens"),
                written,
                read,
                int(r, "output_tokens"),
                0,
            ]
        };
        let (names, starts) = (
            ["starting_at", "group_by[]"],
            [cost_start.into(), usage_start.into()],
        );
        reports(
            "Anthropic",
            &headers,
            [&urls[0], &urls[1]],
            names,
            starts,
            100.0,
            row,
            fetch,
        )
    }

    /// OpenAI's costs report (amounts in dollars) and completions usage report by model.
    pub fn openai_reports(
        key: &str,
        cost_start: &str,
        usage_start: &str,
        fetch: &mut Fetch,
    ) -> Result<Reports, String> {
        let agent = ("User-Agent", "AIUsageTracker/1.0".to_string());
        let headers = [("Authorization", format!("Bearer {key}")), agent];
        let base = "https://api.openai.com/v1/organization";
        let urls = [format!("{base}/costs"), format!("{base}/usage/completions")];
        let row = |r: &Value| {
            let cached = int(r, "input_cached_tokens");
            let input = (int(r, "input_tokens") - cached).max(0);
            [
                input,
                0,
                cached,
                int(r, "output_tokens"),
                int(r, "num_model_requests"),
            ]
        };
        // The original's `to_unix`.
        let unix = |iso: &str| {
            let invalid = || {
                format!(
                    "ValueError: Invalid isoformat string: {}",
                    py_repr(&json!(iso))
                )
            };
            iso_micros(iso)
                .map(|micros| (micros / 1_000_000).to_string())
                .ok_or_else(invalid)
        };
        let starts = [unix(cost_start)?, unix(usage_start)?];
        let names = ["start_time", "group_by"];
        reports(
            "OpenAI",
            &headers,
            [&urls[0], &urls[1]],
            names,
            starts,
            1.0,
            row,
            fetch,
        )
    }

    /// Records the balance in `store()`; `None` forgets it. Without `today_spend`, today's
    /// spend comes from the last report.
    pub fn set_balance(provider: &str, amount: Option<f64>, today_spend: Option<f64>) {
        set_balance_in(&store(), Utc::now(), provider, amount, today_spend);
    }

    /// `set_balance` on the state folder `dir`, at `now`. Reports are daily, so the entry keeps
    /// how much of today's spend the balance already reflected; only later spend is subtracted.
    pub fn set_balance_in(
        dir: &Path,
        now: DateTime<Utc>,
        provider: &str,
        amount: Option<f64>,
        today_spend: Option<f64>,
    ) {
        let path = dir.join("api_balances.json");
        let mut balances = load_json(&path).as_object().cloned().unwrap_or_default();
        if let Some(amount) = amount {
            let spend = today_spend.unwrap_or_else(|| {
                let cache = load_json(&dir.join("api_cache.json"));
                cache[provider]["daily"][utc_day(now)]
                    .as_f64()
                    .unwrap_or(0.0)
            });
            let as_of = now.format(STAMP).to_string();
            let entry = json!({"amount": round_to(amount, 2), "as_of": as_of,
                               "day_baseline": round_to(spend, 4)});
            balances.insert(provider.to_string(), entry);
        } else {
            balances.shift_remove(provider);
        }
        save_json(&path, &Value::Object(balances));
    }

    /// Spend today, this month and over 30 days, spend per day, tokens over 30 days and per
    /// model (most used first).
    pub fn summarize(
        provider: &str,
        daily: &Map<String, Value>,
        tokens: &Map<String, Value>,
        models: &Map<String, Value>,
        now: DateTime<Utc>,
    ) -> Value {
        let (today, last30) = (utc_day(now), utc_day(now - Duration::days(29)));
        let mut t30 = new_tokens().unwrap_or_default();
        for (_, day) in tokens
            .iter()
            .filter(|(d, _)| !d.is_empty() && **d >= last30)
        {
            FIELDS
                .iter()
                .for_each(|f| t30[*f] = json!(int(&t30, f) + int(day, f)));
        }
        // Python's `sum` of nothing is the int 0, and stays one through `round`.
        let since = |start: &str| {
            let days = daily
                .iter()
                .filter(|(d, _)| !d.is_empty() && d.as_str() >= start);
            let add = |sum, (_, v): (_, &Value)| sum + Num::Float(v.as_f64().unwrap_or(0.0));
            rounded(days.fold(Num::Int(0), add), 4)
        };
        let round4 = |v: &Value| json!(round_to(v.as_f64().unwrap_or(0.0), 4));
        let mut days: Vec<_> = daily.iter().filter(|(d, _)| !d.is_empty()).collect();
        days.sort_by_key(|(d, _)| *d);
        let mut rows: Vec<Value> = Vec::new();
        for (model, counts) in models {
            let mut row = json!({ "model": model });
            FIELDS.iter().for_each(|f| row[*f] = json!(int(counts, f)));
            row["total"] = json!(FIELDS[..4].iter().map(|f| int(counts, f)).sum::<i64>());
            rows.push(row);
        }
        rows.sort_by_key(|row| -int(row, "total"));
        json!({
            "provider": provider,
            "today": round4(daily.get(&today).unwrap_or(&json!(0.0))),
            "month": since(&format!("{}01", &today[..8])),
            "last30": since(&last30),
            "daily": days.into_iter().map(|(d, v)| (d.clone(), round4(v))).collect::<Value>(),
            "tokens_30d": t30,
            "models": rows,
        })
    }

    /// One entry per provider: spend, tokens and estimated balance for those with an admin
    /// key, the last good reading when a report fails. `log` gets the progress lines.
    pub fn api_usage(log: &mut dyn FnMut(&str)) -> Value {
        api_usage_at(Utc::now(), &store(), &admin_key, &mut get_json, log)
    }

    /// `api_usage` at `now`, on the state folder `dir`, with the keys and the HTTP call given.
    pub fn api_usage_at(
        now: DateTime<Utc>,
        dir: &Path,
        key_of: &dyn Fn(&str) -> Option<String>,
        fetch: &mut Fetch,
        log: &mut dyn FnMut(&str),
    ) -> Value {
        let balances = load_json(&dir.join("api_balances.json"));
        let cache_path = dir.join("api_cache.json");
        let mut cache = load_json(&cache_path)
            .as_object()
            .cloned()
            .unwrap_or_default();
        let mut entries = Vec::new();
        for (provider, env, console) in PROVIDERS {
            let Some(key) = key_of(env).filter(|key| !key.is_empty()) else {
                entries.push(
                    json!({"provider": provider, "configured": false, "env": env,
                                    "console": console, "balance": null}),
                );
                continue;
            };
            log(&format!("Fetching {provider} API usage ..."));
            let window_start = now - Duration::days(DAYS - 1);
            let entered = balances[provider]["as_of"].as_str().and_then(iso_micros);
            let bal_start = entered
                .and_then(DateTime::from_timestamp_micros)
                .unwrap_or(now);
            let cost_start = window_start
                .min(now.with_day(1).unwrap_or(now))
                .min(bal_start);
            let starts = [cost_start, window_start].map(|t| t.format(DAY_START).to_string());
            let fetcher = if provider == "OpenAI" {
                openai_reports
            } else {
                anthropic_reports
            };
            let mut entry = match fetcher(&key, &starts[0], &starts[1], fetch) {
                Ok([daily, tokens, models]) => {
                    let mut entry = summarize(provider, &daily, &tokens, &models, now);
                    entry["source"] = json!("live");
                    entry["as_of"] = json!(now.format(STAMP).to_string());
                    entry["error"] = Value::Null;
                    let kept = [
                        "daily",
                        "today",
                        "month",
                        "last30",
                        "tokens_30d",
                        "models",
                        "as_of",
                    ];
                    let kept = kept.iter().map(|k| (k.to_string(), entry[*k].clone()));
                    cache.insert(provider.to_string(), kept.collect());
                    log("  ok");
                    entry
                }
                Err(error) => {
                    let error = error.strip_prefix("ApiError: ").unwrap_or(&error);
                    log(&format!("  {error}"));
                    let cached = cache.get(provider).filter(|c| c.is_object() && truthy(c));
                    let mut entry = cached
                        .cloned()
                        .unwrap_or_else(|| json!({"daily": {}, "models": []}));
                    entry["provider"] = json!(provider);
                    entry["error"] = json!(error);
                    entry["source"] = cache.get(provider).map_or(Value::Null, |_| json!("cached"));
                    entry
                }
            };
            entry["configured"] = json!(true);
            entry["env"] = json!(env);
            entry["console"] = json!(console);
            let daily = entry.get("daily").filter(|d| truthy(d)).cloned();
            let daily = daily.unwrap_or_else(|| json!({}));
            entry["balance"] =
                estimate_balance(balances[provider].clone(), daily).unwrap_or_default();
            entries.push(entry);
        }
        save_json(&cache_path, &Value::Object(cache));
        Value::Array(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replays `testdata/python_expected.json`: the scenarios of `test_api_billing.py` and a few
    /// more, as the Python original ran them (`testdata/make_expected.py`), with its clock.
    /// Each request must be the one Python made and gets the page Python got; the entries, the
    /// log lines and the saved files must then be the ones Python produced.
    #[test]
    fn matches_the_original_on_canned_reports() {
        let expected: Value =
            serde_json::from_str(include_str!("../testdata/python_expected.json"))
                .unwrap_or_default();
        let now = DateTime::from_timestamp(1_795_120_882, 0).unwrap_or_default();
        let scenarios = expected.as_object().cloned().unwrap_or_default();
        assert_eq!(scenarios.len(), 10);
        for (name, scenario) in &scenarios {
            let dir =
                std::env::temp_dir().join(format!("api-billing-rs-{}-{name}", std::process::id()));
            for (i, step) in scenario["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let at = format!("{name}, step {i}");
                if let Some(balances) = step.get("write") {
                    native::save_json(&dir.join("api_balances.json"), balances);
                } else if let Some(args) = step.get("balance") {
                    let provider = args[0].as_str().unwrap_or_default();
                    set_balance_in(&dir, now, provider, args[1].as_f64(), args[2].as_f64());
                } else {
                    let mut requests = step["requests"].as_array().into_iter().flatten();
                    let mut fetch = |url: &str, headers: &native::Headers, provider: &str| {
                        let request = requests.next().cloned().unwrap_or_default();
                        let mut sent: Vec<_> = headers.iter().map(|(_, v)| json!(v)).collect();
                        sent.sort_by_key(|v| v.to_string());
                        let made = json!({"url": url, "headers": sent, "provider": provider});
                        for part in ["url", "headers", "provider"] {
                            assert_eq!(made[part], request[part], "{at}");
                        }
                        let failed = request["error"].as_str().map(|e| format!("ApiError: {e}"));
                        failed.map_or_else(|| Ok(request["body"].clone()), Err)
                    };
                    let key_of = |env: &str| step["keys"][env].as_str().map(str::to_string);
                    let mut log = Vec::new();
                    let mut note = |line: &str| log.push(json!(line));
                    let entries = api_usage_at(now, &dir, &key_of, &mut fetch, &mut note);
                    assert_eq!(entries, step["entries"], "{at}");
                    assert_eq!(json!(log), step["log"], "{at}");
                    assert_eq!(requests.next(), None, "{at}");
                }
            }
            for file in ["balances", "cache"] {
                let saved = native::load_json(&dir.join(format!("api_{file}.json")));
                assert_eq!(saved, scenario[file], "{name}: {file}");
            }
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn reads_days_and_reprs_as_python() {
        let day = |bucket: Value| bucket_day(bucket).map(Option::unwrap_or_default);
        assert_eq!(day(json!({"start_time": 0})), Ok("1970-01-01".into()));
        assert_eq!(
            day(json!({"starting_at": "", "start_time": true})),
            Ok("1970-01-01".into())
        );
        assert_eq!(
            day(json!({"starting_at": ["2026-11-19"]})),
            Ok("['2026-11-".into())
        );
        assert_eq!(day(json!({"start_time": ""})), Ok(String::new()));
        assert_eq!(
            native::utc_day(DateTime::from_timestamp(86399, 0).unwrap_or_default()),
            "1970-01-01"
        );
    }
}
