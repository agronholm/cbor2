use pyo3::prelude::*;
use pyo3::{PyErr, PyResult, Python};

pub fn raise_cbor_error<T>(py: Python<'_>, class_name: &str, msg: &str) -> PyResult<T> {
    let exc = py
        .import("cbor2._types")?
        .getattr(class_name)?
        .call1((msg,))?;
    Err(PyErr::from_value(exc))
}

pub fn raise_cbor_error_from<T>(py: Python<'_>, class_name: &str, msg: &str, cause: PyErr) -> PyResult<T> {
    let exc = py
        .import("cbor2._types")?
        .getattr(class_name)?
        .call1((msg,))?;
    let outer = PyErr::from_value(exc);
    outer.set_cause(py, Some(cause));
    Err(outer)
}
