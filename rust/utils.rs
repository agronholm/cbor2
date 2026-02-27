use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::{PyErr, PyResult, Python, import_exception};

pub struct PyImportable {
    lock: PyOnceLock<Py<PyAny>>,
    module: &'static str,
    attribute: &'static str,
}

impl PyImportable {
    // LCOV_EXCL_START
    pub const fn new(module: &'static str, attribute: &'static str) -> Self {
        Self {
            lock: PyOnceLock::new(),
            module,
            attribute,
        }
    }
    // LCOV_EXCL_STOP

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
