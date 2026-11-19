//! The view model: everything the page shows, already worded, coloured and measured.
//!
//! This is `Widget.show` and its helpers from widget.py (`_tool_cost`, `_service_limits`,
//! `_limit_bar`, `_countdown`, `_api_section`, `_show_working`) with the tkinter calls taken
//! out. The page (`ui/app.js`) only turns these structs into elements.
//! `scripts/compare_format.py` checks the result against the labels the Python widget creates.

use crate::core;
use crate::format::{clip, float_or_zero, fmt, money, percent, thousands, until, Num, Zone};
use serde::Serialize;
use serde_json::{Map, Value};

// The palette at the top of widget.py. `ui/style.css` repeats the ones the frame itself uses.
pub const TEXT: &str = "#e6e8eb";
pub const MUTED: &str = "#98a2b3";
pub const WARN: &str = "#f79009";
pub const RED: &str = "#d92d20";
pub const GREEN: &str = "#12b76a";
const TOOL_COLORS: [(&str, &str); 2] = [("Claude Code", "#e8916f"), ("Codex", "#5b8ff0")];
const API_COLORS: [(&str, &str); 2] = [("Anthropic", "#e8916f"), ("OpenAI", "#10a37f")];
/// Tools whose API-equivalent dollar cost gets its own block.
const COST_TOOLS: [&str; 2] = ["Claude Code", "Codex"];

fn color_in(table: &[(&str, &'static str)], name: &str) -> &'static str {
    table
        .iter()
        .find(|(key, _)| *key == name)
        .map_or(MUTED, |(_, color)| color)
}

/// The moment the widget is drawn, and the zone its times are written in.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub now: f64,
    pub zone: Zone,
}

impl Clock {
    pub fn system() -> Clock {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64());
        Clock {
            now,
            zone: Zone::Local,
        }
    }

    fn at(&self, epoch: f64, pattern: &str) -> String {
        self.zone.strftime(epoch, pattern).unwrap_or_default()
    }

    /// `datetime.fromisoformat(...).astimezone()` then `strftime`; empty when unreadable.
    fn iso(&self, text: &Value, pattern: &str) -> String {
        let epoch = text.as_str().and_then(|text| self.zone.parse_iso(text));
        epoch.map_or(String::new(), |epoch| self.at(epoch, pattern))
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Dot {
    pub color: &'static str,
    pub text: String,
}

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct CostBlock {
    pub color: &'static str,
    pub title: String,
    /// Shown instead of the figures when pricing.json has no prices.
    pub hint: Option<String>,
    pub month: String,
    pub today: String,
    pub warning: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Pace {
    pub text: String,
    pub color: &'static str,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct LimitBar {
    pub label: String,
    pub percent: String,
    /// 0..=100, how much of the bar is filled.
    pub fill: f64,
    pub color: &'static str,
    pub reset: String,
    pub pace: Option<Pace>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct LimitBlock {
    pub color: &'static str,
    pub title: String,
    pub plan: Option<String>,
    pub empty: Option<String>,
    pub windows: Vec<LimitBar>,
    pub details: Vec<String>,
    pub note: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub color: &'static str,
    /// "small" or "balance".
    pub kind: &'static str,
}

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct ApiBlock {
    pub color: Option<&'static str>,
    pub title: String,
    pub right: Option<String>,
    pub lines: Vec<Line>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct View {
    pub today: String,
    pub today_split: Vec<Dot>,
    pub month: String,
    pub month_sub: String,
    pub costs: Vec<CostBlock>,
    pub limits: Vec<LimitBlock>,
    /// "Claude Code (this PC): …" and "OpenAI account: …", each possibly two lines.
    pub notes: Vec<String>,
    pub api: Vec<ApiBlock>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub text: String,
    pub color: &'static str,
}

/// One working session in the hover card.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TipRow {
    pub color: &'static str,
    pub title: String,
    pub lines: Vec<String>,
}

/// Python's truth test, for the `if x:` and `x or default` in the original.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `str(v)` for the values that end up as label text.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|found| truthy(found))
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn object<'a>(v: Option<&'a Value>) -> Option<&'a Map<String, Value>> {
    v.and_then(Value::as_object)
}

fn number(v: Option<&Value>) -> Num {
    v.and_then(Num::of).unwrap_or(Num::Int(0))
}

impl View {
    /// Before the first scan has finished.
    pub fn empty() -> View {
        View {
            today: "—".into(),
            today_split: Vec::new(),
            month: "—".into(),
            month_sub: String::new(),
            costs: Vec::new(),
            limits: Vec::new(),
            notes: Vec::new(),
            api: Vec::new(),
        }
    }

    /// `Widget.show(data)`: the whole card for one scan, with countdowns as of `clock.now`.
    pub fn build(data: &Value, clock: &Clock) -> View {
        let today_key = data.get("today").and_then(Value::as_str).unwrap_or("");
        let daily_tool = object(data.get("daily_tool"));
        let today = object(daily_tool.and_then(|days| days.get(today_key)));
        let today_total = Num::sum(today.into_iter().flatten().filter_map(|(_, n)| Num::of(n)));
        let today_split = TOOL_COLORS
            .iter()
            .map(|(tool, color)| Dot {
                color,
                text: format!(
                    "{tool} {}  ",
                    fmt(number(today.and_then(|tools| tools.get(*tool))))
                ),
            })
            .collect();

        let month_start = clip(today_key, 8) + "01";
        let rows: Vec<&Value> = list(data, "day")
            .iter()
            .filter(|row| {
                row.get("key")
                    .and_then(Value::as_str)
                    .is_some_and(|key| key >= month_start.as_str())
            })
            .collect();
        let sum = |field: &str| Num::sum(rows.iter().map(|row| number(row.get(field))));
        let has_pricing = get(data, "has_pricing").is_some();
        let mut month_sub = format!("{} usage records", thousands(sum("calls")));
        if has_pricing {
            month_sub += &format!(" · {} at API prices", money(sum("cost").f()));
        }

        let now = data
            .get("now_epoch")
            .and_then(Value::as_f64)
            .unwrap_or(clock.now);
        let mut notes = Vec::new();
        // Anthropic has no account token history, so Claude's line comes from this PC's logs.
        let claude = list(data, "tool")
            .iter()
            .find(|tool| tool.get("key").and_then(Value::as_str) == Some("Claude Code"));
        if let Some(claude) = claude {
            let latest = daily_tool
                .into_iter()
                .flatten()
                .filter_map(|(day, tools)| Some((day, get(tools, "Claude Code")?)))
                .max_by(|a, b| a.0.cmp(b.0));
            let mut note = format!(
                "Claude Code (this PC): {} lifetime tokens",
                fmt(number(claude.get("total")))
            );
            if let Some((day, tokens)) = latest {
                note += &format!("\nLatest day {day}: {}", fmt(number(Some(tokens))));
            }
            notes.push(note);
        }
        let account = data.get("openai_analytics");
        let lifetime = account
            .and_then(|a| a.get("summary"))
            .and_then(|summary| summary.get("lifetimeTokens"))
            .filter(|tokens| !tokens.is_null());
        if let Some(lifetime) = lifetime {
            let days = object(account.and_then(|a| a.get("daily")));
            let latest = days.into_iter().flatten().max_by(|a, b| a.0.cmp(b.0));
            let mut note = format!(
                "OpenAI account: {} lifetime tokens",
                fmt(number(Some(lifetime)))
            );
            if let Some((day, tokens)) = latest {
                note += &format!("\nLatest day {day}: {}", fmt(number(Some(tokens))));
            }
            notes.push(note);
        }

        View {
            today: fmt(today_total),
            today_split,
            month: fmt(sum("total")),
            month_sub,
            costs: COST_TOOLS
                .iter()
                .map(|tool| tool_cost(tool, data, today_key, &month_start, has_pricing))
                .collect(),
            limits: list(data, "plan_limits")
                .iter()
                .map(|entry| service_limits(entry, now, clock))
                .collect(),
            notes,
            api: api_section(list(data, "api"), clock),
        }
    }

    /// Every piece of text with its colour, in the order widget.py creates its labels. The
    /// differential check compares this list with the Python widget's.
    pub fn labels(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut add = |text: &str, color: &str| out.push((text.to_string(), color.to_string()));
        add(&self.today, TEXT);
        for dot in &self.today_split {
            add("●", dot.color);
            add(&dot.text, MUTED);
        }
        add(&self.month, TEXT);
        add(&self.month_sub, MUTED);
        for cost in &self.costs {
            add("●", cost.color);
            add(&cost.title, MUTED);
            match &cost.hint {
                Some(hint) => add(hint, MUTED),
                None => {
                    add(&cost.month, TEXT);
                    add(" this month", MUTED);
                    add(&cost.today, TEXT);
                }
            }
            if let Some(warning) = &cost.warning {
                add(warning, WARN);
            }
        }
        for block in &self.limits {
            add("●", block.color);
            add(&block.title, MUTED);
            for plain in block.plan.iter().chain(&block.empty) {
                add(plain, MUTED);
            }
            for bar in &block.windows {
                add(&bar.label, TEXT);
                add(&bar.percent, TEXT);
                add(&format!("bar {} {}", bar.fill, bar.color), "");
                add(&bar.reset, MUTED);
                if let Some(pace) = &bar.pace {
                    add(&pace.text, pace.color);
                }
            }
            for plain in block.details.iter().chain(&block.note) {
                add(plain, MUTED);
            }
            if let Some(error) = &block.error {
                add(error, WARN);
            }
        }
        for note in &self.notes {
            add(note, MUTED);
        }
        for block in &self.api {
            if let Some(color) = block.color {
                add("●", color);
            }
            add(&block.title, MUTED);
            if let Some(right) = &block.right {
                add(right, TEXT);
            }
            for line in &block.lines {
                add(&line.text, line.color);
            }
        }
        out
    }
}

/// `_tool_cost`: what this tool's tokens would cost at API prices, today and this month.
fn tool_cost(
    tool: &str,
    data: &Value,
    today: &str,
    month_start: &str,
    has_pricing: bool,
) -> CostBlock {
    let mut block = CostBlock {
        color: color_in(&TOOL_COLORS, tool),
        title: format!("{} COST (API-EQUIVALENT)", tool.to_uppercase()),
        ..CostBlock::default()
    };
    if !has_pricing {
        block.hint = Some("Add prices to pricing.json to see dollars".into());
        return block;
    }
    let daily = object(data.get("daily_tool_cost"));
    let cost = |tools: &Value| tools.get(tool).and_then(Num::of).unwrap_or(Num::Float(0.0));
    let today_cost = daily
        .and_then(|days| days.get(today))
        .map_or(Num::Float(0.0), cost);
    let this_month = |day: &&String| day.as_str() >= month_start;
    let month_cost = Num::sum(
        daily
            .into_iter()
            .flatten()
            .filter(|(day, _)| this_month(day))
            .map(|(_, tools)| cost(tools)),
    );
    block.month = money(month_cost.f());
    block.today = format!("today {}", money(today_cost.f()));
    let unpriced = object(data.get("daily_tool_unpriced"))
        .into_iter()
        .flatten()
        .filter(|(day, _)| this_month(day))
        .any(|(_, tools)| match tools {
            Value::Array(names) => names.iter().any(|name| name.as_str() == Some(tool)),
            Value::Object(names) => names.contains_key(tool),
            Value::String(names) => names.contains(tool),
            _ => false,
        });
    if unpriced {
        block.warning =
            Some("some models have no price in pricing.json and aren't counted".into());
    }
    block
}

/// `_service_limits`: one service's plan, its windows, and where the reading came from.
fn service_limits(entry: &Value, now: f64, clock: &Clock) -> LimitBlock {
    let service = entry.get("service").map(text).unwrap_or_default();
    let windows = list(entry, "windows");
    let mut block = LimitBlock {
        color: color_in(&TOOL_COLORS, &service),
        title: format!("{} LIMITS", service.to_uppercase()),
        plan: get(entry, "plan").map(text),
        empty: windows.is_empty().then(|| "No limit data yet".into()),
        windows: windows.iter().map(|w| limit_bar(w, now, clock)).collect(),
        details: list(entry, "details").iter().map(text).collect(),
        note: None,
        error: get(entry, "error").map(text),
    };
    if let (false, Some(as_of)) = (windows.is_empty(), get(entry, "as_of")) {
        let seen = |pattern| clock.iso(as_of, pattern);
        block.note = Some(match entry.get("source").and_then(Value::as_str) {
            Some("live") => format!("live, checked {}", seen("%H:%M")),
            Some("cached") => format!("last live check {}", seen("%a %H:%M")),
            Some("log") => format!("from last Codex turn, {}", seen("%a %H:%M")),
            _ => String::new(),
        });
    }
    block
}

/// `_limit_bar` with `_countdown`: one limit window.
fn limit_bar(w: &Value, now: f64, clock: &Clock) -> LimitBar {
    let resets_at = get(w, "resets_at").and_then(Value::as_f64);
    let expired = resets_at.is_some_and(|at| at < now);
    let pct = float_or_zero(get(w, "used_percent"));
    let mut reset = match resets_at {
        Some(_) if expired => "Reading expired; current usage unknown".to_string(),
        Some(at) => clock.at(at, "resets %a %H:%M"),
        None => String::new(),
    };
    if let (Some(at), false) = (resets_at, expired) {
        let left = at - clock.now;
        reset = if left > 0.0 {
            format!("{reset} · in {}", until(left))
        } else {
            "Reset passed; refreshing soon".into()
        };
    }
    let projection = get(w, "projection");
    LimitBar {
        label: w.get("label").map_or("?".into(), text),
        percent: format!("{}{}", if expired { "Last " } else { "" }, percent(pct)),
        fill: pct.clamp(0.0, 100.0),
        color: if pct >= 90.0 {
            RED
        } else if pct >= 70.0 {
            WARN
        } else {
            GREEN
        },
        reset,
        pace: projection.map(|p| Pace {
            text: core::pace_text(Some(p), &|t| clock.at(t as f64, "%H:%M")),
            color: if p.get("hits_at").is_some() {
                WARN
            } else {
                MUTED
            },
        }),
    }
}

/// `_api_section`: spend and estimated balance for each provider that has an admin key.
fn api_section(providers: &[Value], clock: &Clock) -> Vec<ApiBlock> {
    let small = |text: String, color| Line {
        text,
        color,
        kind: "small",
    };
    let configured: Vec<&Value> = providers
        .iter()
        .filter(|p| get(p, "configured").is_some())
        .collect();
    if configured.is_empty() {
        let hint =
            "Set ANTHROPIC_ADMIN_KEY or OPENAI_ADMIN_KEY to track API spend (see README)".into();
        return vec![ApiBlock {
            title: "API SPEND".into(),
            lines: vec![small(hint, MUTED)],
            ..ApiBlock::default()
        }];
    }
    let dollars = |v: Option<&Value>| money(number(v).f());
    configured
        .into_iter()
        .map(|p| {
            let provider = p.get("provider").map(text).unwrap_or_default();
            let mut block = ApiBlock {
                color: Some(color_in(&API_COLORS, &provider)),
                title: format!("{} API", provider.to_uppercase()),
                ..ApiBlock::default()
            };
            if let Some(month) = p.get("month").filter(|month| !month.is_null()) {
                block.right = Some(format!("{} this month", dollars(Some(month))));
                block.lines.push(small(
                    format!(
                        "Today {} · last 30 days {} (UTC days)",
                        dollars(p.get("today")),
                        dollars(p.get("last30"))
                    ),
                    MUTED,
                ));
            }
            match get(p, "balance") {
                Some(bal) => {
                    let remaining = number(bal.get("remaining")).f();
                    block.lines.push(Line {
                        text: format!("Balance ≈ {}", money(remaining)),
                        color: if remaining < 5.0 { RED } else { TEXT },
                        kind: "balance",
                    });
                    let when = clock.iso(bal.get("as_of").unwrap_or(&Value::Null), "%b %d");
                    block.lines.push(small(
                        format!(
                            "estimate: {} entered {when} − {} spent since",
                            dollars(bal.get("entered")),
                            dollars(bal.get("spent_since"))
                        ),
                        MUTED,
                    ));
                }
                None => block.lines.push(small(
                    "Right-click → Set API balance… to track remaining credit".into(),
                    MUTED,
                )),
            }
            if let (Some("cached"), Some(as_of)) =
                (p.get("source").and_then(Value::as_str), get(p, "as_of"))
            {
                let when = clock.iso(as_of, "%a %H:%M");
                block
                    .lines
                    .push(small(format!("last good report {when}"), MUTED));
            }
            if let Some(error) = get(p, "error") {
                block.lines.push(small(text(error), WARN));
            }
            block
        })
        .collect()
}

/// The status line after a good scan: "Updated 14:05", plus a note when logs were unreadable.
pub fn updated(data: &Value, clock: &Clock) -> Status {
    let incomplete = get(data, "warnings").is_some();
    Status {
        text: format!(
            "Updated {}{}",
            clock.at(clock.now, "%H:%M"),
            if incomplete {
                " · incomplete logs"
            } else {
                ""
            }
        ),
        color: MUTED,
    }
}

/// The status line after a failed scan (the colour stays as it was, as in the original).
pub fn scan_failed(error: &str) -> String {
    clip(&format!("Scan failed: {error}"), 60)
}

/// `_working`: "● 2 working" for the header, or nothing when no session is working.
pub fn working(active: &Value) -> String {
    let total = Num::sum(
        object(Some(active))
            .into_iter()
            .flatten()
            .filter(|(_, n)| truthy(n))
            .filter_map(|(_, n)| Num::of(n)),
    );
    match total {
        Num::Int(0) => String::new(),
        Num::Int(n) => format!("● {n} working"),
        Num::Float(x) if x == 0.0 => String::new(),
        Num::Float(x) => format!("● {x:?} working"),
    }
}

/// `_show_working`: one row per working session for the hover card.
pub fn tip(
    sessions: &[Value],
    describe: &dyn Fn(&Value) -> Result<String, String>,
) -> Vec<TipRow> {
    sessions
        .iter()
        .filter_map(|info| {
            let described = describe(info).ok()?;
            let mut lines = described.split('\n');
            let tool = info.get("tool").and_then(Value::as_str).unwrap_or("");
            Some(TipRow {
                color: color_in(&TOOL_COLORS, tool),
                title: lines.next().unwrap_or("").to_string(),
                lines: lines.map(|line| line.trim().to_string()).collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use serde_json::json;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../fixtures/sample-full.json")).unwrap()
    }

    fn clock(now: f64) -> Clock {
        Clock {
            now,
            zone: Zone::Fixed(FixedOffset::east_opt(0).unwrap()),
        }
    }

    #[test]
    fn empty_card_before_the_first_scan() {
        let view = View::empty();
        assert_eq!((view.today.as_str(), view.month.as_str()), ("—", "—"));
        assert!(view.costs.is_empty() && view.limits.is_empty() && view.api.is_empty());
    }

    #[test]
    fn fixture_fills_every_part_of_the_card() {
        let data = fixture();
        let now = data["now_epoch"].as_f64().unwrap();
        let view = View::build(&data, &clock(now + 20.0));

        assert!(view.today.ends_with('M'), "{}", view.today);
        assert_eq!(view.today_split.len(), 2);
        assert_eq!(view.today_split[0].color, "#e8916f");
        assert!(view.today_split[0].text.starts_with("Claude Code "));
        assert!(view.today_split[1].text.starts_with("Codex "));
        assert!(view.today_split[1].text.ends_with("  "));
        assert!(view.month_sub.contains(" usage records · $"));
        assert!(view.month_sub.ends_with(" at API prices"));

        assert_eq!(view.costs.len(), 2);
        assert_eq!(view.costs[0].title, "CLAUDE CODE COST (API-EQUIVALENT)");
        assert!(view.costs[0].month.starts_with('$'));
        assert!(view.costs[0].today.starts_with("today $"));
        assert_eq!(view.costs[0].warning, None);
        assert_eq!(view.costs[1].title, "CODEX COST (API-EQUIVALENT)");
        assert_eq!(
            view.costs[1].warning.as_deref(),
            Some("some models have no price in pricing.json and aren't counted")
        );

        assert_eq!(view.limits.len(), 2);
        let claude = &view.limits[0];
        assert_eq!(claude.title, "CLAUDE CODE LIMITS");
        assert_eq!(claude.plan.as_deref(), Some("Max 20x"));
        assert_eq!(claude.windows.len(), 3);
        let session = &claude.windows[0];
        assert_eq!(
            (session.label.as_str(), session.percent.as_str()),
            ("5-hour session", "82%")
        );
        assert_eq!((session.fill, session.color), (82.0, WARN));
        assert!(session.reset.starts_with("resets "), "{}", session.reset);
        assert!(session.reset.ends_with(" · in 2h 13m"), "{}", session.reset);
        let pace = session.pace.as_ref().unwrap();
        assert!(pace.text.starts_with("At this pace: 100% by "));
        assert_eq!(pace.color, WARN);
        let weekly = &claude.windows[1];
        assert_eq!((weekly.percent.as_str(), weekly.color), ("46%", GREEN));
        assert!(weekly.reset.ends_with(" · in 3d 3h"), "{}", weekly.reset);
        assert_eq!(
            weekly.pace,
            Some(Pace {
                text: "On pace for ~61% by reset".into(),
                color: MUTED
            })
        );
        assert_eq!(claude.windows[2].pace, None);
        assert!(claude.note.as_deref().unwrap().starts_with("live, checked "));
        assert_eq!(claude.error, None);

        let codex = &view.limits[1];
        assert_eq!(codex.plan.as_deref(), Some("Pro"));
        assert_eq!(codex.windows[0].percent, "Last 93%");
        assert_eq!(codex.windows[0].color, RED);
        assert_eq!(
            codex.windows[0].reset,
            "Reading expired; current usage unknown"
        );
        assert_eq!(codex.details, ["Credit balance: 42.5"]);
        assert!(codex
            .note
            .as_deref()
            .unwrap()
            .starts_with("last live check "));
        assert_eq!(codex.error.as_deref(), Some("live check failed (offline?)"));

        assert_eq!(view.notes.len(), 2);
        assert!(view.notes[0].starts_with("Claude Code (this PC): "));
        assert!(view.notes[0].contains(" lifetime tokens\nLatest day "));
        assert!(view.notes[1].starts_with("OpenAI account: 1.84B lifetime tokens\nLatest day 20"));
        assert!(view.notes[1].ends_with(": 6.0M"));

        assert_eq!(view.api.len(), 2);
        let anthropic = &view.api[0];
        assert_eq!(anthropic.title, "ANTHROPIC API");
        assert_eq!(anthropic.color, Some("#e8916f"));
        assert_eq!(anthropic.right.as_deref(), Some("$1,284.57 this month"));
        assert_eq!(
            anthropic.lines[0].text,
            "Today $3.42 · last 30 days $2,011.90 (UTC days)"
        );
        assert_eq!(anthropic.lines[1].text, "Balance ≈ $188.63");
        assert_eq!(
            (anthropic.lines[1].color, anthropic.lines[1].kind),
            (TEXT, "balance")
        );
        assert!(anthropic.lines[2].text.starts_with("estimate: $250.00 entered "));
        assert!(anthropic.lines[2].text.ends_with(" − $61.37 spent since"));
        let openai = &view.api[1];
        assert_eq!(openai.color, Some("#10a37f"));
        assert_eq!(openai.lines[1].color, RED);
        assert!(openai.lines[3].text.starts_with("last good report "));
        assert_eq!(
            (openai.lines[4].text.as_str(), openai.lines[4].color),
            ("OpenAI usage report failed (HTTP 500)", WARN)
        );
    }

    #[test]
    fn a_pc_with_no_logins_and_no_prices() {
        let mut data: Value =
            serde_json::from_str(include_str!("../../fixtures/sample-data.json")).unwrap();
        let view = View::build(&data, &clock(0.0));
        assert_eq!(view.limits[0].empty.as_deref(), Some("No limit data yet"));
        assert_eq!(view.limits[0].note, None);
        assert_eq!(
            view.limits[0].error.as_deref(),
            Some("no Claude Code login found")
        );
        assert_eq!(view.api.len(), 1);
        assert_eq!((view.api[0].title.as_str(), view.api[0].color), ("API SPEND", None));
        assert_eq!(view.notes.len(), 1); // no OpenAI account report

        data["has_pricing"] = json!(false);
        let view = View::build(&data, &clock(0.0));
        assert!(view.month_sub.ends_with(" usage records"));
        assert_eq!(
            view.costs[0].hint.as_deref(),
            Some("Add prices to pricing.json to see dollars")
        );
        assert_eq!(view.costs[1].warning, None);
    }

    #[test]
    fn exact_texts_for_a_small_scan() {
        let data = json!({
            "today": "2027-01-15", "now_epoch": 1_800_000_000, "has_pricing": true, "warnings": ["x"],
            "day": [{"key": "2027-01-15", "total": 1250, "cost": 0.125, "calls": 1234},
                    {"key": "2026-12-31", "total": 5_000_000, "cost": 99.0, "calls": 7}],
            "tool": [{"key": "Claude Code", "total": 5_001_250}],
            "daily_tool": {"2026-12-31": {"Claude Code": 5_000_000}, "2027-01-15": {"Claude Code": 1000, "Codex": 250}},
            "daily_tool_cost": {"2026-12-31": {"Claude Code": 99.0}, "2027-01-15": {"Claude Code": 0.125}},
            "daily_tool_unpriced": {"2026-12-31": ["Codex"]},
            "plan_limits": [{"service": "Codex", "source": "log", "as_of": "2027-01-15T07:30:00Z",
                             "windows": [{"label": "5-hour", "used_percent": 69.5, "resets_at": 1_800_003_600},
                                         {"used_percent": null}]}],
            "api": [], "openai_analytics": {"summary": {}, "daily": null},
        });
        let view = View::build(&data, &clock(1_800_000_060.0));
        assert_eq!(view.today, "1.2k");
        assert_eq!(view.today_split[0].text, "Claude Code 1.0k  ");
        assert_eq!(view.today_split[1].text, "Codex 250  ");
        assert_eq!(view.month, "1.2k");
        assert_eq!(view.month_sub, "1,234 usage records · $0.12 at API prices");
        assert_eq!(view.costs[0].month, "$0.12");
        assert_eq!(view.costs[0].today, "today $0.12");
        assert_eq!(view.costs[1].month, "$0.00");
        assert_eq!(view.costs[1].warning, None); // the unpriced day was last month
        let bar = &view.limits[0].windows[0];
        assert_eq!(bar.percent, "70%");
        assert_eq!(bar.color, GREEN); // the colour goes by the exact value, 69.5
        assert_eq!(bar.reset, "resets Fri 09:00 · in 59m");
        let unknown = &view.limits[0].windows[1];
        assert_eq!(
            (unknown.label.as_str(), unknown.percent.as_str(), unknown.reset.as_str()),
            ("?", "0%", "")
        );
        assert_eq!(
            view.limits[0].note.as_deref(),
            Some("from last Codex turn, Fri 07:30")
        );
        assert_eq!(
            view.notes,
            ["Claude Code (this PC): 5.0M lifetime tokens\nLatest day 2027-01-15: 1.0k"]
        );
        assert_eq!(
            updated(&data, &clock(1_800_000_060.0)).text,
            "Updated 08:01 · incomplete logs"
        );

        let passed = View::build(&data, &clock(1_800_003_600.0));
        assert_eq!(
            passed.limits[0].windows[0].reset,
            "Reset passed; refreshing soon"
        );
    }

    #[test]
    fn labels_follow_the_order_of_the_python_widget() {
        let data = fixture();
        let view = View::build(&data, &clock(data["now_epoch"].as_f64().unwrap()));
        let labels = view.labels();
        assert_eq!(labels[0], (view.today.clone(), TEXT.to_string()));
        assert_eq!(labels[1], ("●".to_string(), "#e8916f".to_string()));
        assert!(labels.contains(&("bar 82 #f79009".to_string(), String::new())));
        assert_eq!(
            labels.last().map(|label| label.1.as_str()),
            Some(WARN) // the OpenAI report error closes the card
        );
    }

    #[test]
    fn status_and_working_texts() {
        assert_eq!(
            updated(&json!({"warnings": []}), &clock(1_800_000_000.0)),
            Status {
                text: "Updated 08:00".into(),
                color: MUTED
            }
        );
        let long = "ValueError: ".to_string() + &"x".repeat(80);
        assert_eq!(scan_failed(&long).chars().count(), 60);
        assert_eq!(
            scan_failed("KeyError: 'today'"),
            "Scan failed: KeyError: 'today'"
        );
        assert_eq!(
            working(&json!({"Claude Code": 1, "Codex": 2})),
            "● 3 working"
        );
        assert_eq!(working(&json!({"Claude Code": 0, "Codex": 0})), "");
        assert_eq!(working(&json!({})), "");
    }

    #[test]
    fn hover_card_rows() {
        let data = fixture();
        let sessions = data["_working_details"].as_array().unwrap();
        let rows = tip(sessions, &core::describe_session);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].color, "#e8916f");
        assert_eq!(rows[0].title, "Opus 5.5 med working on usage-tracker");
        assert_eq!(rows[0].lines[0], "“Port the widget to Rust”");
        assert!(rows[0].lines[1].starts_with("Last prompt: make the hover card"));
        assert_eq!(
            rows[0].lines[2],
            "↳ Explore agent · Haiku 5.5: find every strftime call in widget.py"
        );
        assert_eq!(rows[0].lines[3], "Claude Code · last write 8s ago");
        assert_eq!(rows[1].color, "#5b8ff0");
        assert_eq!(rows[1].title, "gpt-6.1-sol high working on photo");
        assert_eq!(tip(&[json!({"tool": "Codex"})], &core::describe_session), []);
    }
}
