use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::{Py, PyAny, pyclass};
use std::str::FromStr;

enum StrError {
    Strict,
    Ignore,
    Replace,
}

impl FromStr for StrError {
    type Err = PyErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "strict" => Ok(StrError::Strict),
            "ignore" => Ok(StrError::Ignore),
            "replace" => Ok(StrError::Replace),
            other => Err(PyValueError::new_err(format!("invalid mode: {other}"))),
        }
    }
}

impl StrError {
    pub fn as_str(&self) -> &str {
        match self {
            StrError::Strict => "strict",
            StrError::Ignore => "ignore",
            StrError::Replace => "replace",
        }
    }
}

#[pyclass(subclass, module = "cbor2")]
pub struct CBORDecoder {
    fp: Option<Py<PyAny>>,
    tag_hook: Option<Py<PyAny>>,
    object_hook: Option<Py<PyAny>>,
    str_errors: StrError,
    buffer: Vec<u8>,
    decode_depth: u32,
}

#[pymethods]
impl CBORDecoder {
    #[new]
    #[pyo3(signature = (
        fp: "typing.IO[bytes]",
        *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        str_errors: "str" = "strict"
    ))]
    pub fn new(
        fp: Option<&Bound<'_, PyAny>>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        str_errors: &str,
    ) -> PyResult<Self> {
        Ok(Self {
            fp: fp.map(|x| x.clone().unbind()),
            tag_hook: tag_hook.map(|x| x.clone().unbind()),
            object_hook: object_hook.map(|x| x.clone().unbind()),
            str_errors: StrError::from_str(str_errors)?,
            buffer: Vec::new(),
            decode_depth: 0,
        })
    }

    #[getter]
    pub fn fp(&self, py: Python<'_>) -> Py<PyAny> {
        match &self.fp {
            Some(fp) => fp.clone_ref(py),
            None => py.None().into(),
        }
    }

    #[setter]
    pub fn set_fp(&mut self, fp: Py<PyAny>) {
        self.fp = Some(fp);
        self.buffer.clear();
    }

    ///  Decode the next value from the stream.
    ///
    ///  :raises CBORDecodeError: if there is any problem decoding the stream
    pub fn decode(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(py.None().into())
    }

    ///  Wrap the given bytestring as a file and call :meth:`decode` with it as
    ///  the argument.
    ///
    ///  This method was intended to be used from the ``tag_hook`` hook when an
    ///  object needs to be decoded separately from the rest but while still
    ///  taking advantage of the shared value registry.
    #[pyo3(signature = (buf: "bytes"))]
    pub fn decode_from_bytes(&mut self, py: Python<'_>, buf: Vec<u8>) -> PyResult<Py<PyAny>> {
        let tag_hook = self.tag_hook.as_ref().map(|x| x.bind(py));
        let object_hook = self.object_hook.as_ref().map(|x| x.bind(py));
        let mut decoder = CBORDecoder::new(None, tag_hook, object_hook, self.str_errors.as_str())?;
        decoder.buffer = buf;
        decoder.decode(py)
    }
}
