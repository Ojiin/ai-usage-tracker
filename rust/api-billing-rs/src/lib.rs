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

fn admin_key_inner(env: Bound<'_, PyAny>) -> Result<Option<String>, String> {
    let env: serde_json::Value = crate::pyconv::FromPy::from_py(&env)?;
    let out = port::admin_key(env)?;
    Ok(out)
}

/// `admin_key`.
#[pyfunction]
#[pyo3(name = "admin_key", signature = (env))]
fn admin_key(env: Bound<'_, PyAny>) -> PyResult<Option<String>> {
    admin_key_inner(env).map_err(error)
}

fn bucket_day_inner(bucket: Bound<'_, PyAny>) -> Result<Option<String>, String> {
    let bucket: serde_json::Value = crate::pyconv::FromPy::from_py(&bucket)?;
    let out = port::bucket_day(bucket)?;
    Ok(out)
}

/// `bucket_day`.
#[pyfunction]
#[pyo3(name = "bucket_day", signature = (bucket))]
fn bucket_day(bucket: Bound<'_, PyAny>) -> PyResult<Option<String>> {
    bucket_day_inner(bucket).map_err(error)
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

fn set_balance_inner(
    provider: Bound<'_, PyAny>,
    amount: Option<f64>,
    today_spend: Option<Bound<'_, PyAny>>,
) -> Result<(), String> {
    let provider: serde_json::Value = crate::pyconv::FromPy::from_py(&provider)?;
    let today_spend: Option<serde_json::Value> = match &today_spend {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    port::set_balance(provider, amount, today_spend)?;
    Ok(())
}

/// `set_balance`.
#[pyfunction]
#[pyo3(name = "set_balance", signature = (provider, amount, today_spend=None))]
fn set_balance(
    provider: Bound<'_, PyAny>,
    amount: Option<f64>,
    today_spend: Option<Bound<'_, PyAny>>,
) -> PyResult<()> {
    set_balance_inner(provider, amount, today_spend).map_err(error)
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

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(admin_key, m)?)?;
    m.add_function(wrap_pyfunction!(bucket_day, m)?)?;
    m.add_function(wrap_pyfunction!(new_tokens, m)?)?;
    m.add_function(wrap_pyfunction!(set_balance, m)?)?;
    m.add_function(wrap_pyfunction!(estimate_balance, m)?)?;
    Ok(())
}
