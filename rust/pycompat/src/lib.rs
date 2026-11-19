//! Python semantics the ports share: types, truthiness, iteration, `str`, `float`, `round`,
//! `json.dumps` and `datetime.fromisoformat`, each with the error Python raises. Values are
//! `serde_json::Value`, as the PyO3 bridges pass them.

#![forbid(unsafe_code)]

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{self, Write};

pub fn ty(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(m) if m.contains_key("$num") => "float",
        Value::Object(_) => "dict",
    }
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

/// Python's `for x in v`: list items, the characters of a string, the keys of a dict.
pub fn iter(v: &Value) -> Result<Vec<Value>, String> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        Value::String(s) => Ok(s.chars().map(|c| json!(c.to_string())).collect()),
        Value::Object(m) if !m.contains_key("$num") => Ok(m.keys().map(|k| json!(k)).collect()),
        other => Err(format!("TypeError: '{}' object is not iterable", ty(other))),
    }
}

/// Python's `v.get(key)`; `None` and a missing key both give `None`.
pub fn get<'a>(v: &'a Value, key: &str) -> Result<Option<&'a Value>, String> {
    match v {
        Value::Object(m) => Ok(m.get(key).filter(|x| !x.is_null())),
        other => Err(format!(
            "AttributeError: '{}' object has no attribute 'get'",
            ty(other)
        )),
    }
}

/// Python's `str` of a scalar; strings as they are.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `json.dumps` separators: `, ` between items and `: ` after a key.
struct PythonSeparators;

impl serde_json::ser::Formatter for PythonSeparators {
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        w.write_all(if first { b"" } else { b", " })
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        w.write_all(if first { b"" } else { b", " })
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
}

/// `json.dumps(v)` as bytes. Text outside ASCII is written as UTF-8, where Python writes
/// `\uXXXX`; both read back the same.
pub fn dumps(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PythonSeparators);
    // Writing a `Value` to memory cannot fail; an empty file reads back as no history.
    if v.serialize(&mut ser).is_err() {
        out.clear();
    }
    out
}

/// `datetime.fromisoformat(s)` as epoch microseconds, for `YYYY-MM-DD`, then any separator,
/// a time and an optional offset. Without an offset the time is local, as in Python. The
/// basic format (`20270115T0730`) is not read.
pub fn iso_micros(s: &str) -> Option<i64> {
    let mut s = s.replace('Z', "+00:00");
    let shape = |i: usize, b: u8| {
        if i == 4 || i == 7 {
            b == b'-'
        } else {
            b.is_ascii_digit()
        }
    };
    if !s.is_ascii() || s.len() < 10 || !s.bytes().take(10).enumerate().all(|(i, b)| shape(i, b)) {
        return None;
    }
    if s.len() > 10 {
        s.replace_range(10..11, "T");
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
        return Some(at.timestamp_micros());
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
    Some(
        Local
            .from_local_datetime(&wall)
            .earliest()?
            .timestamp_micros(),
    )
}

/// Python's `round(x, places)`: the nearest decimal of the exact binary value, ties to even.
pub fn round_to(x: f64, places: usize) -> f64 {
    format!("{x:.places$}")
        .parse()
        .map_or(x, |rounded: f64| rounded)
}

/// Python's `float(v)`.
pub fn to_float(v: &Value) -> Result<f64, String> {
    match v {
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| "OverflowError: int too large to convert to float".into()),
        Value::String(s) => s
            .trim()
            .parse()
            .map_err(|_| format!("ValueError: could not convert string to float: '{s}'")),
        other => Err(format!(
            "TypeError: float() argument must be a string or a real number, not '{}'",
            ty(other)
        )),
    }
}

/// Python's `repr(v)`: `None`, `True`, `1e+21`, `'text'`, `['a', 1]`, `{'k': 'v'}`. Floats the
/// bridge passes as `{"$num": "NaN"}` print as Python prints them.
pub fn py_repr(v: &Value) -> String {
    let items = |a: &mut dyn Iterator<Item = String>| a.collect::<Vec<_>>().join(", ");
    match v {
        Value::String(s) => {
            let quote = if s.contains('\'') && !s.contains('"') {
                '"'
            } else {
                '\''
            };
            let mut out = String::from(quote);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c == quote => out.extend(['\\', c]),
                    c => out.push(c),
                }
            }
            out.push(quote);
            out
        }
        Value::Number(n) if n.is_f64() => {
            let text = format!("{:?}", n.as_f64().unwrap_or_default());
            match text.split_once('e') {
                Some((mantissa, exp)) => {
                    let (sign, digits) = exp.strip_prefix('-').map_or(('+', exp), |d| ('-', d));
                    format!("{mantissa}e{sign}{digits:0>2}")
                }
                None => text,
            }
        }
        Value::Array(a) => format!("[{}]", items(&mut a.iter().map(py_repr))),
        Value::Object(m) => match m.get("$num").and_then(Value::as_str) {
            Some(special) => special
                .replace("Infinity", "inf")
                .replace("NaN", "nan")
                .replace("-0", "-0.0"),
            None => {
                let pair =
                    |(k, v): (&String, &Value)| format!("{}: {}", py_repr(&json!(k)), py_repr(v));
                format!("{{{}}}", items(&mut m.iter().map(pair)))
            }
        },
        scalar => py_str(scalar),
    }
}

/// Python's `str(v)` for any value: text as it is, everything else as `repr` writes it.
pub fn str_of(v: &Value) -> String {
    v.as_str().map_or_else(|| py_repr(v), str::to_string)
}

/// The `AttributeError` Python raises for `v.name`.
pub fn no_attr(v: &Value, name: &str) -> String {
    let ty = ty(v);
    format!("AttributeError: '{ty}' object has no attribute '{name}'")
}

/// Python's `v["key"]` for a string key, with the error each type raises.
pub fn item<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    match v {
        Value::Object(m) if !m.contains_key("$num") => {
            m.get(key).ok_or_else(|| format!("KeyError: {key}"))
        }
        Value::String(_) => Err("TypeError: string indices must be integers, not 'str'".into()),
        Value::Array(_) => {
            Err("TypeError: list indices must be integers or slices, not str".into())
        }
        other => Err(format!(
            "TypeError: '{}' object is not subscriptable",
            ty(other)
        )),
    }
}

/// `urllib.parse.quote_plus(text)`.
pub fn quote_plus(text: &str) -> String {
    let quote = |b: u8| match b {
        b' ' => "+".to_string(),
        b if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) => char::from(b).to_string(),
        b => format!("%{b:02X}"),
    };
    text.bytes().map(quote).collect()
}

/// `json.loads(path.read_text(encoding="utf-8"))`; `None` when the file cannot be read or is
/// not JSON.
pub fn read_json(path: &std::path::Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// `path.write_text(json.dumps(data, indent=1))`, creating the parent folders first.
pub fn write_json_indented(path: &std::path::Path, data: &Value) -> io::Result<()> {
    let mut text = Vec::new();
    let indent = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut text, indent);
    data.serialize(&mut ser)?;
    path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
    std::fs::write(path, text)
}
