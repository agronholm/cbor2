//! Low-level CBOR primitive tokens produced by [`crate::stream_decoder::CBORStreamDecoder`].
//!
//! Each token corresponds to the *head* of a single CBOR data item. Containers
//! (arrays, maps) and indefinite-length strings are represented by "start"
//! tokens followed by their contents and a terminating [`Break`], rather than
//! being assembled into Python objects. This lets callers intercept the raw
//! primitive stream before handing it to the high-level assembler.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

/// A CBOR unsigned or negative integer (major types 0 and 1).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct Integer {
    #[pyo3(get)]
    pub value: Py<PyAny>,
}

#[pymethods]
impl Integer {
    #[new]
    fn new(value: Py<PyAny>) -> Self {
        Integer { value }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("Integer({})", self.value.bind(py).repr()?))
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        match other.cast::<Integer>() {
            Ok(other) => self.value.bind(py).eq(other.get().value.bind(py)),
            Err(_) => Ok(false),
        }
    }
}

/// A definite-length CBOR byte string, or a single chunk of an indefinite-length
/// byte string (major type 2).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct ByteString {
    #[pyo3(get)]
    pub value: Py<PyBytes>,
    /// The CBOR-declared length of the string in bytes.
    #[pyo3(get)]
    pub length: usize,
}

#[pymethods]
impl ByteString {
    #[new]
    #[pyo3(signature = (value, length=0))]
    fn new(value: Py<PyBytes>, length: usize) -> Self {
        ByteString { value, length }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("ByteString({})", self.value.bind(py).repr()?))
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        match other.cast::<ByteString>() {
            Ok(other) => self
                .value
                .bind(py)
                .as_any()
                .eq(other.get().value.bind(py).as_any()),
            Err(_) => Ok(false),
        }
    }
}

/// A definite-length CBOR text string, or a single chunk of an indefinite-length
/// text string (major type 3).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct TextString {
    #[pyo3(get)]
    pub value: Py<PyString>,
    /// The CBOR-declared length of the (encoded) string in bytes.
    #[pyo3(get)]
    pub length: usize,
}

#[pymethods]
impl TextString {
    #[new]
    #[pyo3(signature = (value, length=0))]
    fn new(value: Py<PyString>, length: usize) -> Self {
        TextString { value, length }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("TextString({})", self.value.bind(py).repr()?))
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        match other.cast::<TextString>() {
            Ok(other) => self
                .value
                .bind(py)
                .as_any()
                .eq(other.get().value.bind(py).as_any()),
            Err(_) => Ok(false),
        }
    }
}

/// The start of an indefinite-length byte string (major type 2, subtype 31).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct ByteStringStart;

#[pymethods]
impl ByteStringStart {
    #[new]
    fn new() -> Self {
        ByteStringStart
    }

    fn __repr__(&self) -> &'static str {
        "ByteStringStart()"
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.cast::<ByteStringStart>().is_ok()
    }
}

/// The start of an indefinite-length text string (major type 3, subtype 31).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct TextStringStart;

#[pymethods]
impl TextStringStart {
    #[new]
    fn new() -> Self {
        TextStringStart
    }

    fn __repr__(&self) -> &'static str {
        "TextStringStart()"
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.cast::<TextStringStart>().is_ok()
    }
}

/// The start of a CBOR array (major type 4). ``length`` is :data:`None` for
/// indefinite-length arrays.
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct ArrayStart {
    #[pyo3(get)]
    pub length: Option<usize>,
}

#[pymethods]
impl ArrayStart {
    #[new]
    #[pyo3(signature = (length=None))]
    fn new(length: Option<usize>) -> Self {
        ArrayStart { length }
    }

    fn __repr__(&self) -> String {
        match self.length {
            Some(length) => format!("ArrayStart({length})"),
            None => "ArrayStart(None)".to_string(),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<ArrayStart>() {
            Ok(other) => self.length == other.get().length,
            Err(_) => false,
        }
    }
}

/// The start of a CBOR map (major type 5). ``length`` is the number of key/value
/// *pairs*, or :data:`None` for indefinite-length maps.
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct MapStart {
    #[pyo3(get)]
    pub length: Option<usize>,
}

#[pymethods]
impl MapStart {
    #[new]
    #[pyo3(signature = (length=None))]
    fn new(length: Option<usize>) -> Self {
        MapStart { length }
    }

    fn __repr__(&self) -> String {
        match self.length {
            Some(length) => format!("MapStart({length})"),
            None => "MapStart(None)".to_string(),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<MapStart>() {
            Ok(other) => self.length == other.get().length,
            Err(_) => false,
        }
    }
}

/// A CBOR semantic tag (major type 6). The tagged content follows as subsequent
/// tokens.
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct Tag {
    #[pyo3(get)]
    pub number: u64,
}

#[pymethods]
impl Tag {
    #[new]
    fn new(number: u64) -> Self {
        Tag { number }
    }

    fn __repr__(&self) -> String {
        format!("Tag({})", self.number)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<Tag>() {
            Ok(other) => self.number == other.get().number,
            Err(_) => false,
        }
    }
}

/// A CBOR "simple value" (major type 7, subtypes 0-19 and 24). ``value`` is the
/// raw simple value number.
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct Simple {
    #[pyo3(get)]
    pub value: u8,
}

#[pymethods]
impl Simple {
    #[new]
    fn new(value: u8) -> Self {
        Simple { value }
    }

    fn __repr__(&self) -> String {
        format!("Simple({})", self.value)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<Simple>() {
            Ok(other) => self.value == other.get().value,
            Err(_) => false,
        }
    }
}

/// A CBOR floating point number (major type 7, subtypes 25-27).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct Float {
    #[pyo3(get)]
    pub value: f64,
}

#[pymethods]
impl Float {
    #[new]
    fn new(value: f64) -> Self {
        Float { value }
    }

    fn __repr__(&self) -> String {
        format!("Float({})", self.value)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<Float>() {
            Ok(other) => {
                self.value == other.get().value
                    || (self.value.is_nan() && other.get().value.is_nan())
            }
            Err(_) => false,
        }
    }
}

/// A CBOR boolean (major type 7, subtypes 20 and 21).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct Boolean {
    #[pyo3(get)]
    pub value: bool,
}

#[pymethods]
impl Boolean {
    #[new]
    fn new(value: bool) -> Self {
        Boolean { value }
    }

    fn __repr__(&self) -> String {
        format!("Boolean({})", if self.value { "True" } else { "False" })
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<Boolean>() {
            Ok(other) => self.value == other.get().value,
            Err(_) => false,
        }
    }
}

/// The CBOR null value (major type 7, subtype 22).
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct Null;

#[pymethods]
impl Null {
    #[new]
    fn new() -> Self {
        Null
    }

    fn __repr__(&self) -> &'static str {
        "Null()"
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.cast::<Null>().is_ok()
    }
}

/// The CBOR undefined value (major type 7, subtype 23).
#[pyclass(module = "cbor2.tokens", frozen, name = "Undefined")]
pub struct UndefinedToken;

#[pymethods]
impl UndefinedToken {
    #[new]
    fn new() -> Self {
        UndefinedToken
    }

    fn __repr__(&self) -> &'static str {
        "Undefined()"
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.cast::<UndefinedToken>().is_ok()
    }
}

/// The CBOR "break" stop code (major type 7, subtype 31), terminating an
/// indefinite-length string, array or map.
#[pyclass(module = "cbor2.tokens", frozen)]
pub struct Break;

#[pymethods]
impl Break {
    #[new]
    fn new() -> Self {
        Break
    }

    fn __repr__(&self) -> &'static str {
        "Break()"
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.cast::<Break>().is_ok()
    }
}
