use pyo3::prelude::*;
use pyo3::{import_exception, PyErr, PyResult, Python};
use pyo3::sync::PyOnceLock;

import_exception!(cbor2._types, CBORDecodeError);
import_exception!(cbor2._types, CBORDecodeValueError);
import_exception!(cbor2._types, CBORDecodeTypeError);


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

pub fn wrap_cbor_error<T>(
    py: Python<'_>,
    class_name: &str,
    msg: &str,
    f: impl FnOnce() -> PyResult<T>
) -> PyResult<T> {
    f().map_err(|e| create_cbor_error(py, class_name, msg, Some(e)))
}
