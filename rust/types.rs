use pyo3::exceptions::PyTypeError;
use pyo3::prelude::PyAnyMethods;
use pyo3::types::{PyDict, PyDictMethods, PyFrozenSet, PyIterator, PyString, PyTuple};
use pyo3::{Bound, Py, PyAny, PyResult, Python, pyclass, pymethods};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Represents a CBOR semantic tag.
///
/// :param int tag: tag number
/// :param value: encapsulated value (any object)
#[pyclass(get_all, frozen, module = "cbor2")]
pub struct CBORTag {
    pub tag: u64,
    pub value: Py<PyAny>,
}

#[pymethods]
impl CBORTag {
    #[new]
    pub fn new(tag: Bound<'_, PyAny>, value: Bound<'_, PyAny>) -> PyResult<Self> {
        let tag: u64 = tag.extract().map_err(|_| {
            PyTypeError::new_err("CBORTag tags must be positive integers less than 2**64")
        })?;
        Ok(Self {
            tag,
            value: value.unbind(),
        })
    }
}

/// Represents a CBOR "simple value".
///
/// :param int value: the value (0-255)
#[pyclass(get_all, frozen, eq, ord, str = "{value}", module = "cbor2")]
#[derive(PartialEq, PartialOrd, Hash)]
pub struct CBORSimpleValue {
    pub value: u8,
}

#[pymethods]
impl CBORSimpleValue {
    #[new]
    pub fn new(value: u8) -> Self {
        Self { value }
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.value.hash(&mut hasher);
        hasher.finish()
    }
}

/// A hashable, immutable mapping type.
///
/// The arguments to ``FrozenDict`` are processed just like those to ``dict``.
#[pyclass(mapping, module = "cbor2")]
pub struct FrozenDict {
    dict: Py<PyDict>,
    hash: Option<u64>,
}

#[pymethods]
impl FrozenDict {
    #[new]
    #[pyo3(signature = (*args))]
    pub fn new(args: &Bound<'_, PyTuple>) -> PyResult<Self> {
        let dict = PyDict::from_sequence(args)?.unbind();
        Ok(Self { dict, hash: None })
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        self.dict.as_any().bind(py).try_iter()
    }

    fn __len__(&self, py: Python<'_>) -> usize {
        self.dict.bind(py).len()
    }

    fn __getitem__<'py>(
        &self,
        py: Python<'py>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.dict.bind(py).get_item(key)
    }

    fn __repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyString>> {
        self.dict.bind(py).repr()
    }

    fn __hash__(&mut self, py: Python<'_>) -> PyResult<u64> {
        if (self.hash).is_none() {
            let keys_hash = PyFrozenSet::new(py, self.dict.bind(py).keys())?.hash()?;
            let values_hash = PyFrozenSet::new(py, self.dict.bind(py).values())?.hash()?;
            let mut hasher = DefaultHasher::new();
            hasher.write_isize(keys_hash);
            hasher.write_isize(values_hash);
            self.hash = Some(hasher.finish());
        }
        Ok(self.hash.unwrap())
    }
}

#[pyclass(frozen, module = "cbor2")]
pub struct UndefinedType;

#[pymethods]
impl UndefinedType {
    fn __repr__(&self) -> &str {
        "undefined"
    }

    fn __bool__(&self) -> bool {
        false
    }
}

#[pyclass(frozen, module = "cbor2")]
pub struct BreakMarkerType;

#[pymethods]
impl BreakMarkerType {
    fn __repr__(&self) -> &str {
        "break_marker"
    }

    fn __bool__(&self) -> bool {
        true
    }
}
