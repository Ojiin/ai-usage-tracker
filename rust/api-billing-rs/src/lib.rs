//! Python bindings for `api_billing.py`: generated from the signatures in `port.rs` by rust-anything.
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

fn admin_key_inner(env: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let env: serde_json::Value = crate::pyconv::FromPy::from_py(&env)?;
    let out = port::admin_key(env)?;
    Ok(out)
}

/// `admin_key`.
#[pyfunction]
#[pyo3(name = "admin_key", signature = (env))]
fn admin_key(py: Python<'_>, env: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    admin_key_inner(env)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn utc_day_inner(dt: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let dt: serde_json::Value = crate::pyconv::FromPy::from_py(&dt)?;
    let out = port::utc_day(dt)?;
    Ok(out)
}

/// `utc_day`.
#[pyfunction]
#[pyo3(name = "utc_day", signature = (dt))]
fn utc_day(py: Python<'_>, dt: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    utc_day_inner(dt)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn load_json_inner(path: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let path: serde_json::Value = crate::pyconv::FromPy::from_py(&path)?;
    let out = port::load_json(path)?;
    Ok(out)
}

/// `load_json`.
#[pyfunction]
#[pyo3(name = "load_json", signature = (path))]
fn load_json(py: Python<'_>, path: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    load_json_inner(path)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn save_json_inner(
    path: Bound<'_, PyAny>,
    data: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let path: serde_json::Value = crate::pyconv::FromPy::from_py(&path)?;
    let data: serde_json::Value = crate::pyconv::FromPy::from_py(&data)?;
    let out = port::save_json(path, data)?;
    Ok(out)
}

/// `save_json`.
#[pyfunction]
#[pyo3(name = "save_json", signature = (path, data))]
fn save_json(
    py: Python<'_>,
    path: Bound<'_, PyAny>,
    data: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    save_json_inner(path, data)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn get_json_inner(
    url: Bound<'_, PyAny>,
    headers: Bound<'_, PyAny>,
    provider: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let url: serde_json::Value = crate::pyconv::FromPy::from_py(&url)?;
    let headers: serde_json::Value = crate::pyconv::FromPy::from_py(&headers)?;
    let provider: serde_json::Value = crate::pyconv::FromPy::from_py(&provider)?;
    let out = port::get_json(url, headers, provider)?;
    Ok(out)
}

/// `get_json`.
#[pyfunction]
#[pyo3(name = "get_json", signature = (url, headers, provider))]
fn get_json(
    py: Python<'_>,
    url: Bound<'_, PyAny>,
    headers: Bound<'_, PyAny>,
    provider: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    get_json_inner(url, headers, provider)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn bucket_day_inner(bucket: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let bucket: serde_json::Value = crate::pyconv::FromPy::from_py(&bucket)?;
    let out = port::bucket_day(bucket)?;
    Ok(out)
}

/// `bucket_day`.
#[pyfunction]
#[pyo3(name = "bucket_day", signature = (bucket))]
fn bucket_day(py: Python<'_>, bucket: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    bucket_day_inner(bucket)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn new_tokens_inner() -> Result<serde_json::Value, String> {
    let out = port::new_tokens()?;
    Ok(out)
}

/// `new_tokens`.
#[pyfunction]
#[pyo3(name = "new_tokens", signature = ())]
fn new_tokens(py: Python<'_>) -> PyResult<Py<PyAny>> {
    new_tokens_inner()
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn anthropic_reports_inner(
    key: Bound<'_, PyAny>,
    cost_start: Bound<'_, PyAny>,
    usage_start: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let key: serde_json::Value = crate::pyconv::FromPy::from_py(&key)?;
    let cost_start: serde_json::Value = crate::pyconv::FromPy::from_py(&cost_start)?;
    let usage_start: serde_json::Value = crate::pyconv::FromPy::from_py(&usage_start)?;
    let out = port::anthropic_reports(key, cost_start, usage_start)?;
    Ok(out)
}

/// `anthropic_reports`.
#[pyfunction]
#[pyo3(name = "anthropic_reports", signature = (key, cost_start, usage_start))]
fn anthropic_reports(
    py: Python<'_>,
    key: Bound<'_, PyAny>,
    cost_start: Bound<'_, PyAny>,
    usage_start: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    anthropic_reports_inner(key, cost_start, usage_start)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn openai_reports_inner(
    key: Bound<'_, PyAny>,
    cost_start: Bound<'_, PyAny>,
    usage_start: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let key: serde_json::Value = crate::pyconv::FromPy::from_py(&key)?;
    let cost_start: serde_json::Value = crate::pyconv::FromPy::from_py(&cost_start)?;
    let usage_start: serde_json::Value = crate::pyconv::FromPy::from_py(&usage_start)?;
    let out = port::openai_reports(key, cost_start, usage_start)?;
    Ok(out)
}

/// `openai_reports`.
#[pyfunction]
#[pyo3(name = "openai_reports", signature = (key, cost_start, usage_start))]
fn openai_reports(
    py: Python<'_>,
    key: Bound<'_, PyAny>,
    cost_start: Bound<'_, PyAny>,
    usage_start: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    openai_reports_inner(key, cost_start, usage_start)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn set_balance_inner(
    provider: Bound<'_, PyAny>,
    amount: f64,
    today_spend: Option<Bound<'_, PyAny>>,
) -> Result<serde_json::Value, String> {
    let provider: serde_json::Value = crate::pyconv::FromPy::from_py(&provider)?;
    let today_spend: Option<serde_json::Value> = match &today_spend {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::set_balance(provider, amount, today_spend)?;
    Ok(out)
}

/// `set_balance`.
#[pyfunction]
#[pyo3(name = "set_balance", signature = (provider, amount, today_spend=None))]
fn set_balance(
    py: Python<'_>,
    provider: Bound<'_, PyAny>,
    amount: f64,
    today_spend: Option<Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    set_balance_inner(provider, amount, today_spend)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn estimate_balance_inner(
    entry: Bound<'_, PyAny>,
    daily: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let entry: serde_json::Value = crate::pyconv::FromPy::from_py(&entry)?;
    let daily: serde_json::Value = crate::pyconv::FromPy::from_py(&daily)?;
    let out = port::estimate_balance(entry, daily)?;
    Ok(out)
}

/// `estimate_balance`.
#[pyfunction]
#[pyo3(name = "estimate_balance", signature = (entry, daily))]
fn estimate_balance(
    py: Python<'_>,
    entry: Bound<'_, PyAny>,
    daily: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    estimate_balance_inner(entry, daily)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn summarize_inner(
    provider: Bound<'_, PyAny>,
    daily: Bound<'_, PyAny>,
    tokens: Bound<'_, PyAny>,
    models: Bound<'_, PyAny>,
    now: f64,
) -> Result<serde_json::Value, String> {
    let provider: serde_json::Value = crate::pyconv::FromPy::from_py(&provider)?;
    let daily: serde_json::Value = crate::pyconv::FromPy::from_py(&daily)?;
    let tokens: serde_json::Value = crate::pyconv::FromPy::from_py(&tokens)?;
    let models: serde_json::Value = crate::pyconv::FromPy::from_py(&models)?;
    let out = port::summarize(provider, daily, tokens, models, now)?;
    Ok(out)
}

/// `summarize`.
#[pyfunction]
#[pyo3(name = "summarize", signature = (provider, daily, tokens, models, now))]
fn summarize(
    py: Python<'_>,
    provider: Bound<'_, PyAny>,
    daily: Bound<'_, PyAny>,
    tokens: Bound<'_, PyAny>,
    models: Bound<'_, PyAny>,
    now: f64,
) -> PyResult<Py<PyAny>> {
    summarize_inner(provider, daily, tokens, models, now)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(admin_key, m)?)?;
    m.add_function(wrap_pyfunction!(utc_day, m)?)?;
    m.add_function(wrap_pyfunction!(load_json, m)?)?;
    m.add_function(wrap_pyfunction!(save_json, m)?)?;
    m.add_function(wrap_pyfunction!(get_json, m)?)?;
    m.add_function(wrap_pyfunction!(bucket_day, m)?)?;
    m.add_function(wrap_pyfunction!(new_tokens, m)?)?;
    m.add_function(wrap_pyfunction!(anthropic_reports, m)?)?;
    m.add_function(wrap_pyfunction!(openai_reports, m)?)?;
    m.add_function(wrap_pyfunction!(set_balance, m)?)?;
    m.add_function(wrap_pyfunction!(estimate_balance, m)?)?;
    m.add_function(wrap_pyfunction!(summarize, m)?)?;
    Ok(())
}
