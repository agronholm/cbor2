use pyo3::basic::CompareOp;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::PyAnyMethods;
use pyo3::types::{
    PyDict, PyDictMethods, PyFrozenSet, PyInt, PyIterator, PyNotImplemented, PyString,
    PyTuple,
};
use pyo3::{
    pyclass, pymethods, Bound, IntoPyObjectExt, Py, PyAny, PyResult, PyTypeInfo, Python,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Represents a CBOR semantic tag.
///
/// :param int tag: tag number
/// :param value: encapsulated value (any object)
#[pyclass(get_all, module = "cbor2")]
pub struct CBORTag {
    pub tag: u64,
    pub value: Py<PyAny>,
}

impl CBORTag {
    pub fn new_internal(tag: u64, value: Bound<'_, PyAny>) -> Self {
        Self { tag, value: value.unbind() }
    }
}

#[pymethods]
impl CBORTag {
    #[new]
    pub fn new(tag: Bound<'_, PyAny>, value: Bound<'_, PyAny>) -> PyResult<Self> {
        let tag: u64 = tag.extract().map_err(|_| {
            PyTypeError::new_err("CBORTag tags must be positive integers less than 2**64")
        })?;
        Ok(Self::new_internal(tag, value))
    }

    fn __richcmp__<'py>(
        &self,
        py: Python<'py>,
        other: &Bound<'py, PyAny>,
        op: CompareOp,
    ) -> PyResult<Bound<'py, PyAny>> {
        if let Ok(other) = other.cast::<CBORTag>() {
            let other_tag = other.borrow().tag;
            if self.tag != other_tag {
                return op.matches(self.tag.cmp(&other_tag)).into_bound_py_any(py)
            }
            let borrowed_other = other.borrow();
            let bound_self = self.value.bind(py);
            let bound_other = borrowed_other.value.bind(py);
            let compare_result = match op {
                CompareOp::Eq => bound_self.eq(bound_other),
                CompareOp::Ne => bound_self.ne(bound_other),
                CompareOp::Lt => bound_self.lt(bound_other),
                CompareOp::Le => bound_self.le(bound_other),
                CompareOp::Gt => bound_self.gt(bound_other),
                CompareOp::Ge => bound_self.ge(bound_other),
            }?;
            compare_result.into_bound_py_any(py)
        } else {
            // Non-comparable types: signal NotImplemented to Python
            PyNotImplemented::get(py).into_bound_py_any(py)
        }
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<u64> {
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(self.tag);
        match self.value.call_method0(py, "__hash__") {
            Ok(value_hash) => {
                hasher.write_isize(value_hash.extract(py)?);
                Ok(hasher.finish())
            },
            Err(cause) => {
                let exc = PyRuntimeError::new_err("This CBORTag is not hashable because its value is not hashable");
                exc.set_cause(py, Some(cause));
                Err(exc)
            }
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "CBORTag({}, {})",
            self.tag,
            self.value.bind(py).repr()?
        ))
    }
}

/// Represents a CBOR "simple value".
///
/// :param int value: the value (0-255)
#[pyclass(frozen, str = "{0}", module = "cbor2")]
#[derive(PartialEq, PartialOrd, Hash)]
pub struct CBORSimpleValue(pub u8);

#[pymethods]
impl CBORSimpleValue {
    #[new]
    pub fn new(value: Bound<'_, PyInt>) -> PyResult<Self> {
        if let Ok(integer) = value.extract::<u8>()
            && !(24..32).contains(&integer)
        {
            Ok(Self(integer))
        } else {
            Err(PyValueError::new_err(
                "simple value out of range (0..23, 32..255)",
            ))
        }
    }

    #[getter]
    fn value(&self) -> u8 {
        self.0
    }

    fn __richcmp__<'py>(
        &self,
        py: Python<'py>,
        other: &Bound<'py, PyAny>,
        op: CompareOp,
    ) -> PyResult<Bound<'py, PyAny>> {
        if let Ok(other) = other.cast::<CBORSimpleValue>() {
            let other_value = other.borrow().0;
            op.matches(self.0.cmp(&other_value)).into_bound_py_any(py)
        } else if let Ok(other) = other.extract::<u8>() {
            op.matches(self.0.cmp(&other)).into_bound_py_any(py)
        } else {
            // Non-comparable types: signal NotImplemented to Python
            PyNotImplemented::get(py).into_bound_py_any(py)
        }
    }

    fn __repr__(&self) -> String {
        format!("CBORSimpleValue({})", self.0)
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
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
        let dict_type = <PyDict as PyTypeInfo>::type_object(args.py());
        let dict: Py<PyDict> = dict_type.call1(args)?.cast_into()?.unbind();
        Ok(Self { dict, hash: None })
    }

    fn keys<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.dict.bind(py).call_method0("keys")
    }

    fn items<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.dict.bind(py).call_method0("items")
    }

    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.dict.bind(py).call_method0("values")
    }

    fn get<'py>(&self, py: Python<'py>, key: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        self.dict.bind(py).call_method1("get", (key,))
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.dict.bind(py).eq(other)
    }

    fn __contains__(&self, py: Python<'_>, key: Bound<'_, PyAny>) -> PyResult<bool> {
        self.dict.bind(py).contains(key)
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        self.dict.bind(py).try_iter()
    }

    fn __len__(&self, py: Python<'_>) -> usize {
        self.dict.bind(py).len()
    }

    fn __getitem__<'py>(
        &self,
        py: Python<'py>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.dict.bind(py).call_method1("__getitem__", (key,))
    }

    fn __repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyString>> {
        let dict_repr = self.dict.bind(py).repr()?;
        Ok(PyString::new(
            py,
            format!("FrozenDict({})", dict_repr).as_str(),
        ))
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
