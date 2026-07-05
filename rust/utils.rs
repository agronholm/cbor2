use crate::types::{CBORDecodeEOF, CBORDecodeError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::{PyErr, PyErrArguments, PyResult, Python};
use std::fmt::Display;

pub struct PyImportable {
    lock: PyOnceLock<Py<PyAny>>,
    module: &'static str,
    attribute: &'static str,
}

impl PyImportable {
    pub const fn new(module: &'static str, attribute: &'static str) -> Self {
        Self {
            lock: PyOnceLock::new(),
            module,
            attribute,
        }
    }

    pub fn get<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let class = self.lock.get_or_try_init(py, || {
            let mut value = py.import(self.module)?.into_any();
            for part in self.attribute.split('.') {
                value = value.getattr(part)?;
            }
            Ok::<_, PyErr>(value.unbind())
        })?;
        Ok(class.clone_ref(py).into_bound(py))
    }
}

pub fn create_exc_from(py: Python<'_>, exc: PyErr, cause: Option<PyErr>) -> PyErr {
    exc.set_cause(py, cause);
    exc
}

pub fn raise_exc_from<T>(py: Python<'_>, exc: PyErr, cause: Option<PyErr>) -> PyResult<T> {
    Err(create_exc_from(py, exc, cause))
}

/// Wraps an error raised while decoding an item of the given ``typename`` in a
/// [`CBORDecodeError`], preserving [`CBORDecodeEOF`] errors as-is and attaching
/// non-CBOR errors as the cause.
pub fn wrap_decode_error(py: Python<'_>, err: PyErr, typename: &dyn Display) -> PyErr {
    if err.is_instance_of::<CBORDecodeEOF>(py) {
        err
    } else if err.is_instance_of::<CBORDecodeError>(py) {
        CBORDecodeError::new_err(format!(
            "error decoding {}: {}",
            typename,
            err.arguments(py)
        ))
    } else {
        create_exc_from(
            py,
            CBORDecodeError::new_err(format!("error decoding {}", typename)),
            Some(err),
        )
    }
}
