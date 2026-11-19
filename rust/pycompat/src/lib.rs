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

/// Python's `c.isspace()`: Unicode white space plus the separators U+001C to U+001F.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Python's `text.split()`: the words between runs of white space.
pub fn split_ws(text: &str) -> Vec<&str> {
    text.split(is_space).filter(|w| !w.is_empty()).collect()
}

/// Python's `text.splitlines()`: lines end at `\n`, `\r`, `\r\n`, `\v`, `\f`, U+001C to U+001E,
/// U+0085, U+2028 and U+2029; there is no empty last line.
pub fn splitlines(text: &str) -> Vec<&str> {
    let ends = |c: char| "\n\r\x0b\x0c\x1c\x1d\x1e\u{85}\u{2028}\u{2029}".contains(c);
    let (mut lines, mut rest) = (Vec::new(), text);
    while let Some(at) = rest.find(ends) {
        lines.push(&rest[..at]);
        let end = if rest[at..].starts_with("\r\n") {
            2
        } else {
            rest[at..].chars().next().map_or(1, char::len_utf8)
        };
        rest = &rest[at + end..];
    }
    if !rest.is_empty() {
        lines.push(rest);
    }
    lines
}

/// Python's `text.capitalize()`: the first character in title case (`ß` gives `Ss`, the
/// digraph `ǆ` gives `ǅ`), the rest lower case.
pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    let title = match chars.next() {
        None => return String::new(),
        Some('Ǆ' | 'ǅ' | 'ǆ') => "ǅ".to_string(),
        Some('Ǉ' | 'ǈ' | 'ǉ') => "ǈ".to_string(),
        Some('Ǌ' | 'ǋ' | 'ǌ') => "ǋ".to_string(),
        Some('Ǳ' | 'ǲ' | 'ǳ') => "ǲ".to_string(),
        Some(first) => {
            let upper: String = first.to_uppercase().collect();
            let mut upper = upper.chars();
            let head: String = upper.next().into_iter().collect();
            head + &upper.as_str().to_lowercase()
        }
    };
    title + &chars.as_str().to_lowercase()
}

/// `json.dumps(v)` byte for byte: `, ` and `: ` separators, text outside printable ASCII as
/// `\uXXXX`, floats as `repr` writes them.
pub fn dumps_ascii(v: &Value) -> String {
    fn text(s: &str, out: &mut String) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                ' '..='~' => out.push(c),
                c => {
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        out.push_str(&format!("\\u{unit:04x}"));
                    }
                }
            }
        }
        out.push('"');
    }
    fn write(v: &Value, out: &mut String) {
        match v {
            Value::String(s) => text(s, out),
            Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    out.push_str(if i == 0 { "" } else { ", " });
                    write(x, out);
                }
                out.push(']');
            }
            Value::Object(m) => {
                out.push('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    out.push_str(if i == 0 { "" } else { ", " });
                    text(k, out);
                    out.push_str(": ");
                    write(x, out);
                }
                out.push('}');
            }
            Value::Number(n) if n.is_f64() => out.push_str(&py_repr(v)),
            scalar => out.push_str(&scalar.to_string()),
        }
    }
    let mut out = String::new();
    write(v, &mut out);
    out
}

/// `PureWindowsPath(text).name`: the last folder or file name, without drive or share.
pub fn windows_name(text: &str) -> String {
    let path = text.replace('/', "\\");
    let tail = match path.strip_prefix("\\\\") {
        Some(unc) => unc.splitn(3, '\\').nth(2).unwrap_or(""),
        None if path.chars().nth(1) == Some(':') => {
            path.char_indices().nth(2).map_or("", |(at, _)| &path[at..])
        }
        None => &path,
    };
    let name = tail
        .split('\\')
        .rfind(|part| !part.is_empty() && *part != ".");
    name.unwrap_or("").to_string()
}

/// `PurePosixPath(text).name`.
pub fn posix_name(text: &str) -> String {
    let name = text
        .split('/')
        .rfind(|part| !part.is_empty() && *part != ".");
    name.unwrap_or("").to_string()
}

/// Python's `repr(x)` of a float: `1e+21`, `0.1`, `nan`, `inf`.
pub fn float_repr(x: f64) -> String {
    if x.is_finite() {
        py_repr(&json!(x))
    } else {
        format!("{x}").to_lowercase()
    }
}

/// Python's `int(x)` of a float: toward zero, with the error for NaN and infinity.
pub fn int_of(x: f64) -> Result<i64, String> {
    if x.is_nan() {
        Err("ValueError: cannot convert float NaN to integer".into())
    } else if x.is_infinite() {
        Err("OverflowError: cannot convert float infinity to integer".into())
    } else {
        Ok(x.trunc() as i64)
    }
}

/// Python's `x // y` for floats and a positive `y`.
pub fn float_floor_div(x: f64, y: f64) -> f64 {
    let rest = x % y;
    let div = (x - if rest < 0.0 { rest + y } else { rest }) / y;
    let floor = div.floor();
    if div - floor > 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// What tells two values apart as members of a Python set or keys of a dict, with the error
/// for a list or dict.
pub fn hash_key(v: &Value) -> Result<String, String> {
    match v {
        Value::Array(_) | Value::Object(_) => {
            Err(format!("TypeError: unhashable type: '{}'", ty(v)))
        }
        other => Ok(other.to_string()),
    }
}

/// `PurePath(name).stem`: the name without its last suffix.
pub fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if !name[..dot].trim_start_matches('.').is_empty() => &name[..dot],
        _ => name,
    }
}

/// `path.name`, empty when there is none.
pub fn name_of(path: &std::path::Path) -> String {
    let name = path.file_name();
    name.map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// `path.with_suffix(suffix)`.
pub fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    path.with_file_name(format!("{}{suffix}", stem(&name_of(path))))
}

/// `root.rglob(pattern)` for a pattern on the name alone, in pathlib's order (Python 3.13): a
/// folder's matches, then its subfolders' matches as each is listed, the subfolder listed last
/// walked first. Links to folders are not followed.
pub fn rglob(root: &std::path::Path, matches: &dyn Fn(&str) -> bool) -> Vec<std::path::PathBuf> {
    let list = |dir: &std::path::Path| -> Vec<(std::path::PathBuf, bool, bool)> {
        let entry = |e: std::fs::DirEntry| {
            let matched = matches(&e.file_name().to_string_lossy());
            (e.path(), matched, e.file_type().is_ok_and(|t| t.is_dir()))
        };
        let entries = std::fs::read_dir(dir).ok().into_iter().flatten();
        entries.flatten().map(entry).collect()
    };
    let found = |dir: &std::path::Path| list(dir).into_iter().filter(|e| e.1).map(|e| e.0);
    let (mut out, mut stack) = (found(root).collect::<Vec<_>>(), vec![root.to_path_buf()]);
    while let Some(dir) = stack.pop() {
        for (path, _, _) in list(&dir).into_iter().filter(|e| e.2) {
            out.extend(found(&path));
            stack.push(path);
        }
    }
    out
}
