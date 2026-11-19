//! Python bindings for `tracker.py`: generated from the signatures in `port.rs` by rust-anything.
//! Regenerate with `rust-anything refresh` after changing them.

use pyo3::prelude::*;

pub mod js;
pub mod port;
pub mod pyconv;
pub mod pynum;

#[allow(unused_imports)]
use pynum::Num;

fn error(message: String) -> PyErr {
    use pyo3::exceptions as e;
    let (kind, text) = message.split_once(": ").unwrap_or(("", message.as_str()));
    let text = text.to_string();
    match kind {
        "TypeError" => e::PyTypeError::new_err(text),
        "KeyError" => e::PyKeyError::new_err(text),
        "IndexError" => e::PyIndexError::new_err(text),
        "OverflowError" => e::PyOverflowError::new_err(text),
        "ZeroDivisionError" => e::PyZeroDivisionError::new_err(text),
        "AttributeError" => e::PyAttributeError::new_err(text),
        "RuntimeError" => e::PyRuntimeError::new_err(text),
        "NotImplementedError" => e::PyNotImplementedError::new_err(text),
        "LookupError" => e::PyLookupError::new_err(text),
        "ArithmeticError" => e::PyArithmeticError::new_err(text),
        "UnicodeError" => e::PyUnicodeError::new_err(text),
        "ValueError" => e::PyValueError::new_err(text),
        _ => e::PyValueError::new_err(message),
    }
}

fn local_day_inner(ts: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let ts: serde_json::Value = crate::pyconv::FromPy::from_py(&ts)?;
    let out = port::local_day(ts)?;
    Ok(out)
}

/// `local_day`.
#[pyfunction]
#[pyo3(name = "local_day", signature = (ts))]
fn local_day(py: Python<'_>, ts: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    local_day_inner(ts)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn local_time_inner(ts: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let ts: serde_json::Value = crate::pyconv::FromPy::from_py(&ts)?;
    let out = port::local_time(ts)?;
    Ok(out)
}

/// `local_time`.
#[pyfunction]
#[pyo3(name = "local_time", signature = (ts))]
fn local_time(py: Python<'_>, ts: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    local_time_inner(ts)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn claude_tool_events_inner(
    name: Bound<'_, PyAny>,
    args: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let name: serde_json::Value = crate::pyconv::FromPy::from_py(&name)?;
    let args: serde_json::Value = crate::pyconv::FromPy::from_py(&args)?;
    let out = port::claude_tool_events(name, args)?;
    Ok(out)
}

/// `claude_tool_events`.
#[pyfunction]
#[pyo3(name = "claude_tool_events", signature = (name, args))]
fn claude_tool_events(
    py: Python<'_>,
    name: Bound<'_, PyAny>,
    args: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    claude_tool_events_inner(name, args)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn scan_commands_inner(events: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let events: serde_json::Value = crate::pyconv::FromPy::from_py(&events)?;
    let out = port::scan_commands(events)?;
    Ok(out)
}

/// `scan_commands`.
#[pyfunction]
#[pyo3(name = "scan_commands", signature = (events))]
fn scan_commands(py: Python<'_>, events: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    scan_commands_inner(events)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn working_now_inner(
    window: i64,
    now: Option<Bound<'_, PyAny>>,
) -> Result<serde_json::Value, String> {
    let now: Option<serde_json::Value> = match &now {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::working_now(window, now)?;
    Ok(out)
}

/// `working_now`.
#[pyfunction]
#[pyo3(name = "working_now", signature = (window=120, now=None))]
fn working_now(py: Python<'_>, window: i64, now: Option<Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
    working_now_inner(window, now)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn tail_rows_inner(
    path: Bound<'_, PyAny>,
    size: Option<Bound<'_, PyAny>>,
) -> Result<serde_json::Value, String> {
    let path: serde_json::Value = crate::pyconv::FromPy::from_py(&path)?;
    let size: Option<serde_json::Value> = match &size {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::tail_rows(path, size)?;
    Ok(out)
}

/// `tail_rows`.
#[pyfunction]
#[pyo3(name = "tail_rows", signature = (path, size=None))]
fn tail_rows(
    py: Python<'_>,
    path: Bound<'_, PyAny>,
    size: Option<Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    tail_rows_inner(path, size)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn pretty_model_inner(model: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let model: serde_json::Value = crate::pyconv::FromPy::from_py(&model)?;
    let out = port::pretty_model(model)?;
    Ok(out)
}

/// `pretty_model`.
#[pyfunction]
#[pyo3(name = "pretty_model", signature = (model))]
fn pretty_model(py: Python<'_>, model: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    pretty_model_inner(model)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn short_text_inner(text: Bound<'_, PyAny>, limit: i64) -> Result<serde_json::Value, String> {
    let text: serde_json::Value = crate::pyconv::FromPy::from_py(&text)?;
    let out = port::short_text(text, limit)?;
    Ok(out)
}

/// `short_text`.
#[pyfunction]
#[pyo3(name = "short_text", signature = (text, limit=90))]
fn short_text(py: Python<'_>, text: Bound<'_, PyAny>, limit: i64) -> PyResult<Py<PyAny>> {
    short_text_inner(text, limit)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn codex_titles_inner() -> Result<serde_json::Value, String> {
    let out = port::codex_titles()?;
    Ok(out)
}

/// `codex_titles`.
#[pyfunction]
#[pyo3(name = "codex_titles", signature = ())]
fn codex_titles(py: Python<'_>) -> PyResult<Py<PyAny>> {
    codex_titles_inner()
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn codex_db_working_inner(now: f64) -> Result<serde_json::Value, String> {
    let out = port::codex_db_working(now)?;
    Ok(out)
}

/// `codex_db_working`.
#[pyfunction]
#[pyo3(name = "codex_db_working", signature = (now))]
fn codex_db_working(py: Python<'_>, now: f64) -> PyResult<Py<PyAny>> {
    codex_db_working_inner(now)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn claude_session_info_inner(path: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let path: serde_json::Value = crate::pyconv::FromPy::from_py(&path)?;
    let out = port::claude_session_info(path)?;
    Ok(out)
}

/// `claude_session_info`.
#[pyfunction]
#[pyo3(name = "claude_session_info", signature = (path))]
fn claude_session_info(py: Python<'_>, path: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    claude_session_info_inner(path)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn working_details_inner(
    window: i64,
    now: Option<Bound<'_, PyAny>>,
) -> Result<serde_json::Value, String> {
    let now: Option<serde_json::Value> = match &now {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::working_details(window, now)?;
    Ok(out)
}

/// `working_details`.
#[pyfunction]
#[pyo3(name = "working_details", signature = (window=120, now=None))]
fn working_details(
    py: Python<'_>,
    window: i64,
    now: Option<Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    working_details_inner(window, now)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn describe_session_inner(info: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let info: serde_json::Value = crate::pyconv::FromPy::from_py(&info)?;
    let out = port::describe_session(info)?;
    Ok(out)
}

/// `describe_session`.
#[pyfunction]
#[pyo3(name = "describe_session", signature = (info))]
fn describe_session(py: Python<'_>, info: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    describe_session_inner(info)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn scan_claude_inner(events: Option<Bound<'_, PyAny>>) -> Result<serde_json::Value, String> {
    let events: Option<serde_json::Value> = match &events {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::scan_claude(events)?;
    Ok(out)
}

/// `scan_claude`.
#[pyfunction]
#[pyo3(name = "scan_claude", signature = (events=None))]
fn scan_claude(py: Python<'_>, events: Option<Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
    scan_claude_inner(events)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn scan_codex_inner(events: Option<Bound<'_, PyAny>>) -> Result<serde_json::Value, String> {
    let events: Option<serde_json::Value> = match &events {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::scan_codex(events)?;
    Ok(out)
}

/// `scan_codex`.
#[pyfunction]
#[pyo3(name = "scan_codex", signature = (events=None))]
fn scan_codex(py: Python<'_>, events: Option<Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
    scan_codex_inner(events)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn to_epoch_inner(iso: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let iso: serde_json::Value = crate::pyconv::FromPy::from_py(&iso)?;
    let out = port::to_epoch(iso)?;
    Ok(out)
}

/// `to_epoch`.
#[pyfunction]
#[pyo3(name = "to_epoch", signature = (iso))]
fn to_epoch(py: Python<'_>, iso: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    to_epoch_inner(iso)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn pretty_plan_inner(raw: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let raw: serde_json::Value = crate::pyconv::FromPy::from_py(&raw)?;
    let out = port::pretty_plan(raw)?;
    Ok(out)
}

/// `pretty_plan`.
#[pyfunction]
#[pyo3(name = "pretty_plan", signature = (raw))]
fn pretty_plan(py: Python<'_>, raw: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    pretty_plan_inner(raw)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn window_label_inner(minutes: f64) -> Result<serde_json::Value, String> {
    let out = port::window_label(minutes)?;
    Ok(out)
}

/// `window_label`.
#[pyfunction]
#[pyo3(name = "window_label", signature = (minutes))]
fn window_label(py: Python<'_>, minutes: f64) -> PyResult<Py<PyAny>> {
    window_label_inner(minutes)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn codex_live_inner() -> Result<serde_json::Value, String> {
    let out = port::codex_live()?;
    Ok(out)
}

/// `codex_live`.
#[pyfunction]
#[pyo3(name = "codex_live", signature = ())]
fn codex_live(py: Python<'_>) -> PyResult<Py<PyAny>> {
    codex_live_inner()
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn codex_details_inner(body: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let body: serde_json::Value = crate::pyconv::FromPy::from_py(&body)?;
    let out = port::codex_details(body)?;
    Ok(out)
}

/// `codex_details`.
#[pyfunction]
#[pyo3(name = "codex_details", signature = (body))]
fn codex_details(py: Python<'_>, body: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    codex_details_inner(body)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn claude_live_inner() -> Result<serde_json::Value, String> {
    let out = port::claude_live()?;
    Ok(out)
}

/// `claude_live`.
#[pyfunction]
#[pyo3(name = "claude_live", signature = ())]
fn claude_live(py: Python<'_>) -> PyResult<Py<PyAny>> {
    claude_live_inner()
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn codex_log_limits_inner(snapshot: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let snapshot: serde_json::Value = crate::pyconv::FromPy::from_py(&snapshot)?;
    let out = port::codex_log_limits(snapshot)?;
    Ok(out)
}

/// `codex_log_limits`.
#[pyfunction]
#[pyo3(name = "codex_log_limits", signature = (snapshot))]
fn codex_log_limits(py: Python<'_>, snapshot: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    codex_log_limits_inner(snapshot)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn load_pricing_inner() -> Result<serde_json::Value, String> {
    let out = port::load_pricing()?;
    Ok(out)
}

/// `load_pricing`.
#[pyfunction]
#[pyo3(name = "load_pricing", signature = ())]
fn load_pricing(py: Python<'_>) -> PyResult<Py<PyAny>> {
    load_pricing_inner()
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn cost_of_inner(
    rec: Bound<'_, PyAny>,
    pricing: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let rec: serde_json::Value = crate::pyconv::FromPy::from_py(&rec)?;
    let pricing: serde_json::Value = crate::pyconv::FromPy::from_py(&pricing)?;
    let out = port::cost_of(rec, pricing)?;
    Ok(out)
}

/// `cost_of`.
#[pyfunction]
#[pyo3(name = "cost_of", signature = (rec, pricing))]
fn cost_of(
    py: Python<'_>,
    rec: Bound<'_, PyAny>,
    pricing: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    cost_of_inner(rec, pricing)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn aggregate_inner(
    records: Bound<'_, PyAny>,
    pricing: Bound<'_, PyAny>,
    events: Option<Bound<'_, PyAny>>,
) -> Result<serde_json::Value, String> {
    let records: serde_json::Value = crate::pyconv::FromPy::from_py(&records)?;
    let pricing: serde_json::Value = crate::pyconv::FromPy::from_py(&pricing)?;
    let events: Option<serde_json::Value> = match &events {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::aggregate(records, pricing, events)?;
    Ok(out)
}

/// `aggregate`.
#[pyfunction]
#[pyo3(name = "aggregate", signature = (records, pricing, events=None))]
fn aggregate(
    py: Python<'_>,
    records: Bound<'_, PyAny>,
    pricing: Bound<'_, PyAny>,
    events: Option<Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    aggregate_inner(records, pricing, events)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(local_day, m)?)?;
    m.add_function(wrap_pyfunction!(local_time, m)?)?;
    m.add_function(wrap_pyfunction!(claude_tool_events, m)?)?;
    m.add_function(wrap_pyfunction!(scan_commands, m)?)?;
    m.add_function(wrap_pyfunction!(working_now, m)?)?;
    m.add_function(wrap_pyfunction!(tail_rows, m)?)?;
    m.add_function(wrap_pyfunction!(pretty_model, m)?)?;
    m.add_function(wrap_pyfunction!(short_text, m)?)?;
    m.add_function(wrap_pyfunction!(codex_titles, m)?)?;
    m.add_function(wrap_pyfunction!(codex_db_working, m)?)?;
    m.add_function(wrap_pyfunction!(claude_session_info, m)?)?;
    m.add_function(wrap_pyfunction!(working_details, m)?)?;
    m.add_function(wrap_pyfunction!(describe_session, m)?)?;
    m.add_function(wrap_pyfunction!(scan_claude, m)?)?;
    m.add_function(wrap_pyfunction!(scan_codex, m)?)?;
    m.add_function(wrap_pyfunction!(to_epoch, m)?)?;
    m.add_function(wrap_pyfunction!(pretty_plan, m)?)?;
    m.add_function(wrap_pyfunction!(window_label, m)?)?;
    m.add_function(wrap_pyfunction!(codex_live, m)?)?;
    m.add_function(wrap_pyfunction!(codex_details, m)?)?;
    m.add_function(wrap_pyfunction!(claude_live, m)?)?;
    m.add_function(wrap_pyfunction!(codex_log_limits, m)?)?;
    m.add_function(wrap_pyfunction!(load_pricing, m)?)?;
    m.add_function(wrap_pyfunction!(cost_of, m)?)?;
    m.add_function(wrap_pyfunction!(aggregate, m)?)?;
    Ok(())
}
