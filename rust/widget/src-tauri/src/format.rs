//! The text and number rules of widget.py, kept identical to the Python ones.
//!
//! `scripts/compare_format.py` runs thousands of inputs through these and through the
//! originals in widget.py; change them only together with that check.

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::Value;

/// A Python number: sums of ints stay exact ints, and turn into a float once a float joins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    Int(i128),
    Float(f64),
}

impl Num {
    /// A JSON number (or `True`/`False`, which Python adds as 1/0); anything else is no number.
    pub fn of(v: &Value) -> Option<Num> {
        match v {
            Value::Bool(b) => Some(Num::Int(i128::from(*b))),
            Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
                (Some(i), _, _) => Some(Num::Int(i128::from(i))),
                (_, Some(u), _) => Some(Num::Int(i128::from(u))),
                (_, _, f) => f.map(Num::Float),
            },
            _ => None,
        }
    }

    pub fn f(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(x) => x,
        }
    }

    /// Python's `+`.
    pub fn plus(self, other: Num) -> Num {
        match (self, other) {
            (Num::Int(a), Num::Int(b)) => a
                .checked_add(b)
                .map_or(Num::Float(a as f64 + b as f64), Num::Int),
            (a, b) => Num::Float(a.f() + b.f()),
        }
    }

    /// Python's `sum(...)`, which starts from the int 0.
    pub fn sum(items: impl IntoIterator<Item = Num>) -> Num {
        items.into_iter().fold(Num::Int(0), Num::plus)
    }
}

/// `fmt` (widget.py:38): 1234567 -> "1.2M", 950 -> "950".
pub fn fmt(n: Num) -> String {
    let x = n.f();
    for (size, suffix, places) in [(1e9, "B", 2), (1e6, "M", 1), (1e3, "k", 1)] {
        if x >= size {
            return format!("{:.places$}{suffix}", x / size);
        }
    }
    match n {
        Num::Int(i) => i.to_string(),
        // str(int(x)): towards zero, and never "-0"
        Num::Float(x) if x.trunc() == 0.0 => "0".into(),
        Num::Float(x) => format!("{:.0}", x.trunc()),
    }
}

/// `until` (widget.py:45): compact time left, "2h 14m", "3d 4h", "12m".
pub fn until(seconds: f64) -> String {
    // int() cuts towards zero, // rounds down, and max() then drops anything negative
    let m = (seconds.trunc() as i64).div_euclid(60).max(0);
    if m >= 1440 {
        format!("{}d {}h", m / 1440, m % 1440 / 60)
    } else if m >= 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

/// `money` (widget.py:53): "$1,234.50", "-$3.20".
pub fn money(n: f64) -> String {
    if n < 0.0 {
        format!("-${}", grouped(&format!("{:.2}", -n)))
    } else {
        format!("${}", grouped(&format!("{n:.2}")))
    }
}

/// Python's `f"{n:,}"` for an int, and near enough for the float nobody should send.
pub fn thousands(n: Num) -> String {
    match n {
        Num::Int(i) => grouped(&i.to_string()),
        Num::Float(x) => grouped(&format!("{x:?}")),
    }
}

/// Adds Python's `,` grouping to the whole part of a formatted number.
fn grouped(number: &str) -> String {
    let (sign, rest) = match number.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", number),
    };
    let (whole, tail) = rest.split_at(rest.find('.').unwrap_or(rest.len()));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return number.to_lowercase(); // "inf", "nan": Python writes them in lower case
    }
    let mut out = String::from(sign);
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out + tail
}

/// `f"{pct:.0f}%"`.
pub fn percent(pct: f64) -> String {
    format!("{pct:.0}%")
}

/// Python's `text[:limit]`, which counts characters, not bytes.
pub fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Python's `float(v or 0)` for the values the services send.
pub fn float_or_zero(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        Some(v) => Num::of(v).map_or(0.0, Num::f),
        None => 0.0,
    }
}

/// Where "local time" is: the PC's zone, or a fixed one so tests and the differential check
/// give the same answer everywhere.
#[derive(Clone, Copy, Debug)]
pub enum Zone {
    Local,
    Fixed(FixedOffset),
}

impl Zone {
    /// `datetime.fromtimestamp(epoch).strftime(pattern)`; `None` for a time no calendar has.
    pub fn strftime(self, epoch: f64, pattern: &str) -> Option<String> {
        if !epoch.is_finite() {
            return None;
        }
        let secs = epoch.floor() as i64;
        Some(match self {
            Zone::Local => Local
                .timestamp_opt(secs, 0)
                .single()?
                .format(pattern)
                .to_string(),
            Zone::Fixed(offset) => offset
                .timestamp_opt(secs, 0)
                .single()?
                .format(pattern)
                .to_string(),
        })
    }

    /// `datetime.fromisoformat(text.replace("Z", "+00:00"))` as epoch seconds. A time without
    /// an offset is local, as `.astimezone()` takes it in Python.
    pub fn parse_iso(self, text: &str) -> Option<f64> {
        let mut s = text.replace('Z', "+00:00");
        let date_shaped = s.is_ascii()
            && s.len() >= 10
            && s.bytes().take(10).enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            });
        if !date_shaped {
            return None;
        }
        if s.len() > 10 {
            s.replace_range(10..11, "T"); // Python takes any separator here
        }
        let aware = [
            "%Y-%m-%dT%H:%M:%S%.f%:z",
            "%Y-%m-%dT%H:%M%:z",
            "%Y-%m-%dT%H:%M:%S%.f%z",
        ];
        if let Some(at) = aware
            .iter()
            .find_map(|f| DateTime::parse_from_str(&s, f).ok())
        {
            return Some(at.timestamp() as f64);
        }
        let naive = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"];
        let wall = match naive
            .iter()
            .find_map(|f| NaiveDateTime::parse_from_str(&s, f).ok())
        {
            Some(wall) => wall,
            None => NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)?,
        };
        let secs = match self {
            Zone::Local => Local.from_local_datetime(&wall).earliest()?.timestamp(),
            Zone::Fixed(offset) => offset.from_local_datetime(&wall).earliest()?.timestamp(),
        };
        Some(secs as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn int(i: i64) -> Num {
        Num::Int(i128::from(i))
    }

    #[test]
    fn fmt_matches_python() {
        let cases = [
            (int(0), "0"),
            (int(999), "999"),
            (int(-5), "-5"),
            (int(1000), "1.0k"),
            (int(1250), "1.2k"), // 1.25 is a tie: Python rounds it to even
            (int(1350), "1.4k"),
            (int(999_949), "999.9k"),
            (int(999_950), "1000.0k"),
            (int(1_000_000), "1.0M"),
            (int(7_208_780), "7.2M"),
            (int(999_999_999), "1000.0M"),
            (int(1_843_250_112), "1.84B"),
            (Num::Float(999.99), "999"),
            (Num::Float(-0.5), "0"),
            (Num::Float(-3.9), "-3"),
            (Num::Float(1500.0), "1.5k"),
        ];
        for (n, expected) in cases {
            assert_eq!(fmt(n), expected, "{n:?}");
        }
    }

    #[test]
    fn until_matches_python() {
        let cases = [
            (0.0, "0m"),
            (59.9, "0m"),
            (60.0, "1m"),
            (3599.0, "59m"),
            (3600.0, "1h 00m"),
            (8040.0, "2h 14m"),
            (86399.0, "23h 59m"),
            (86400.0, "1d 0h"),
            (273_600.0, "3d 4h"),
            (-1.0, "0m"),
            (-0.5, "0m"),
            (-7200.0, "0m"),
        ];
        for (seconds, expected) in cases {
            assert_eq!(until(seconds), expected, "{seconds}");
        }
    }

    #[test]
    fn money_matches_python() {
        let cases = [
            (0.0, "$0.00"),
            (5.4436, "$5.44"),
            (0.125, "$0.12"), // tie, to even
            (0.375, "$0.38"),
            (1284.5731, "$1,284.57"),
            (1_234_567.891, "$1,234,567.89"),
            (999.999, "$1,000.00"),
            (-3.2, "-$3.20"),
            (-1234.5, "-$1,234.50"),
            (-0.001, "-$0.00"),
            (-0.0, "$-0.00"), // not below zero, so Python prints the float's own sign
        ];
        for (n, expected) in cases {
            assert_eq!(money(n), expected, "{n}");
        }
        assert_eq!(money(f64::INFINITY), "$inf");
        assert_eq!(money(f64::NAN), "$nan");
    }

    #[test]
    fn thousands_percent_and_clip() {
        assert_eq!(thousands(int(590)), "590");
        assert_eq!(thousands(int(1234567)), "1,234,567");
        assert_eq!(thousands(int(-1234)), "-1,234");
        assert_eq!(thousands(Num::Float(1234.0)), "1,234.0");
        assert_eq!(percent(82.0), "82%");
        assert_eq!(percent(46.4), "46%");
        assert_eq!(percent(0.5), "0%");
        assert_eq!(percent(1.5), "2%");
        assert_eq!(percent(2.5), "2%");
        assert_eq!(clip("Scan failed: é…x", 15), "Scan failed: é…");
    }

    #[test]
    fn sums_like_python() {
        let values = [json!(5895780), json!(1313000)];
        assert_eq!(
            Num::sum(values.iter().filter_map(Num::of)),
            Num::Int(7_208_780)
        );
        let mixed = [json!(1), json!(0.5), json!(true)];
        assert_eq!(Num::sum(mixed.iter().filter_map(Num::of)), Num::Float(2.5));
        assert_eq!(Num::of(&json!("12")), None);
        assert_eq!(float_or_zero(Some(&json!("82"))), 82.0);
        assert_eq!(float_or_zero(Some(&json!(null))), 0.0);
        assert_eq!(float_or_zero(Some(&json!(46.4))), 46.4);
        assert_eq!(float_or_zero(None), 0.0);
    }

    #[test]
    fn times_in_a_fixed_zone() {
        let zone = Zone::Fixed(FixedOffset::east_opt(2 * 3600).unwrap_or_else(|| unreachable!()));
        let epoch = zone.parse_iso("2027-01-15T08:00:00Z");
        assert_eq!(epoch, Some(1_800_000_000.0));
        assert_eq!(
            zone.parse_iso("2027-01-15T08:00:00.000+00:00"),
            Some(1_800_000_000.0)
        );
        assert_eq!(
            zone.parse_iso("2027-01-15 10:00:00"),
            Some(1_800_000_000.0)
        );
        assert_eq!(zone.parse_iso("2027-01-15T10:00"), Some(1_800_000_000.0));
        assert_eq!(zone.parse_iso("yesterday"), None);
        assert_eq!(
            zone.strftime(1_800_000_000.0, "resets %a %H:%M").as_deref(),
            Some("resets Fri 10:00")
        );
        assert_eq!(
            zone.strftime(1_800_000_000.0, "%b %d").as_deref(),
            Some("Jan 15")
        );
        assert_eq!(zone.strftime(f64::NAN, "%H:%M"), None);
    }
}
