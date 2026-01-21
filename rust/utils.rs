use pyo3::prelude::*;
use pyo3::{PyErr, PyResult, Python};

pub fn create_cbor_error(
    py: Python<'_>,
    class_name: &str,
    msg: &str,
    cause: Option<PyErr>,
) -> PyErr {
    let exc = match py
        .import("cbor2._types")
        .and_then(|m| m.getattr(class_name))
        .and_then(|cls| cls.call1((msg,)))
    {
        Err(e) => e,
        Ok(e) => PyErr::from_value(e),
    };
    exc.set_cause(py, cause);
    exc
}

pub fn raise_cbor_error<T>(py: Python<'_>, class_name: &str, msg: &str) -> PyResult<T> {
    Err(create_cbor_error(py, class_name, msg, None))
}

pub fn raise_cbor_error_from<T>(
    py: Python<'_>,
    class_name: &str,
    msg: &str,
    cause: PyErr,
) -> PyResult<T> {
    Err(create_cbor_error(py, class_name, msg, Some(cause)))
}
