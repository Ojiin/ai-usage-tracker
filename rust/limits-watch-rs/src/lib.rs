//! Python bindings for `limits_watch.py`: generated from the signatures in `port.rs` by rust-anything.
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

fn to_epoch_inner(iso: Bound<'_, PyAny>) -> Result<Option<i64>, String> {
    let iso: serde_json::Value = crate::pyconv::FromPy::from_py(&iso)?;
    let out = port::to_epoch(iso)?;
    Ok(out)
}

/// `to_epoch`.
#[pyfunction]
#[pyo3(name = "to_epoch", signature = (iso))]
fn to_epoch(iso: Bound<'_, PyAny>) -> PyResult<Option<i64>> {
    to_epoch_inner(iso).map_err(error)
}

fn same_window_inner(a: Option<f64>, b: Option<f64>) -> Result<bool, String> {
    let out = port::same_window(a, b)?;
    Ok(out)
}

/// `same_window`.
#[pyfunction]
#[pyo3(name = "same_window", signature = (a, b))]
fn same_window(a: Option<f64>, b: Option<f64>) -> PyResult<bool> {
    same_window_inner(a, b).map_err(error)
}

fn lookback_inner(now: f64, resets_at: f64) -> Result<i64, String> {
    let out = port::lookback(now, resets_at)?;
    Ok(out)
}

/// `lookback`.
#[pyfunction]
#[pyo3(name = "lookback", signature = (now, resets_at))]
fn lookback(now: f64, resets_at: f64) -> PyResult<i64> {
    lookback_inner(now, resets_at).map_err(error)
}

fn project_inner(
    samples: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
    resets_at: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let samples: serde_json::Value = crate::pyconv::FromPy::from_py(&samples)?;
    let now: Num = crate::pyconv::FromPy::from_py(&now)?;
    let resets_at: Num = crate::pyconv::FromPy::from_py(&resets_at)?;
    let out = port::project(samples, now, resets_at)?;
    Ok(out)
}

/// `project`.
#[pyfunction]
#[pyo3(name = "project", signature = (samples, now, resets_at))]
fn project(
    py: Python<'_>,
    samples: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
    resets_at: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    project_inner(samples, now, resets_at)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn window_key_inner(entry: Bound<'_, PyAny>, w: Bound<'_, PyAny>) -> Result<String, String> {
    let entry: serde_json::Value = crate::pyconv::FromPy::from_py(&entry)?;
    let w: serde_json::Value = crate::pyconv::FromPy::from_py(&w)?;
    let out = port::window_key(entry, w)?;
    Ok(out)
}

/// `window_key`.
#[pyfunction]
#[pyo3(name = "window_key", signature = (entry, w))]
fn window_key(entry: Bound<'_, PyAny>, w: Bound<'_, PyAny>) -> PyResult<String> {
    window_key_inner(entry, w).map_err(error)
}

fn record_and_project_inner(
    entries: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
    path: Option<Bound<'_, PyAny>>,
) -> Result<serde_json::Value, String> {
    let entries: serde_json::Value = crate::pyconv::FromPy::from_py(&entries)?;
    let now: Num = crate::pyconv::FromPy::from_py(&now)?;
    let path: Option<serde_json::Value> = match &path {
        None => None,
        Some(v) => Some(crate::pyconv::FromPy::from_py(v)?),
    };
    let out = port::record_and_project(entries, now, path)?;
    Ok(out)
}

/// `record_and_project`.
#[pyfunction]
#[pyo3(name = "record_and_project", signature = (entries, now, path=None))]
fn record_and_project(
    py: Python<'_>,
    entries: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
    path: Option<Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    record_and_project_inner(entries, now, path)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(to_epoch, m)?)?;
    m.add_function(wrap_pyfunction!(same_window, m)?)?;
    m.add_function(wrap_pyfunction!(lookback, m)?)?;
    m.add_function(wrap_pyfunction!(project, m)?)?;
    m.add_function(wrap_pyfunction!(window_key, m)?)?;
    m.add_function(wrap_pyfunction!(record_and_project, m)?)?;
    Ok(())
}
