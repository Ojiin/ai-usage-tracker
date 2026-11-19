//! Python bindings for `openai_analytics.py`: generated from the signatures in `port.rs` by rust-anything.
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

fn codex_executable_inner() -> Result<Option<String>, String> {
    let out = port::codex_executable()?;
    Ok(out)
}

/// `codex_executable`.
#[pyfunction]
#[pyo3(name = "codex_executable", signature = ())]
fn codex_executable() -> PyResult<Option<String>> {
    codex_executable_inner().map_err(error)
}

fn read_account_inner(timeout: i64) -> Result<serde_json::Value, String> {
    let out = port::read_account(timeout)?;
    Ok(out)
}

/// `read_account`.
#[pyfunction]
#[pyo3(name = "read_account", signature = (timeout=25))]
fn read_account(py: Python<'_>, timeout: i64) -> PyResult<Py<PyAny>> {
    read_account_inner(timeout)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn normalize_inner(body: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let body: serde_json::Value = crate::pyconv::FromPy::from_py(&body)?;
    let out = port::normalize(body)?;
    Ok(out)
}

/// `normalize`.
#[pyfunction]
#[pyo3(name = "normalize", signature = (body))]
fn normalize(py: Python<'_>, body: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    normalize_inner(body)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(codex_executable, m)?)?;
    m.add_function(wrap_pyfunction!(read_account, m)?)?;
    m.add_function(wrap_pyfunction!(normalize, m)?)?;
    Ok(())
}
