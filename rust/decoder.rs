use crate::types::BreakMarkerType;
use crate::utils::{raise_cbor_error, raise_cbor_error_from};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyInt, PyString};
use pyo3::{FromPyObject, IntoPyObjectExt, Py, PyAny, intern, pyclass};
use std::cmp::{max, min};
use std::collections::HashMap;
use std::hash::Hash;
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

#[pyclass(module = "cbor2")]
pub struct CBORDecoder {
    pub fp: Option<Py<PyAny>>,
    pub tag_hook: Option<Py<PyAny>>,
    pub object_hook: Option<Py<PyAny>>,
    pub str_errors: StrError,
    pub read_size: u32,

    major_decoders: HashMap<u8, Py<PyAny>>,
    semantic_decoders: HashMap<u64, Py<PyAny>>,
    buffer: Box<Vec<u8>>,
    decode_depth: u32,
    share_index: Option<usize>,
    shareables: Vec<Py<PyAny>>,
}

impl CBORDecoder {
    fn read_to_buffer(&mut self, py: Python<'_>, minimum_amount: usize) -> PyResult<()> {
        let bytes_to_read = max(minimum_amount, self.read_size as usize);
        let fp = match self.fp.as_ref() {
            None => return raise_cbor_error(py, "CBORDecodeError", "no file pointer"),
            Some(fp) => fp.bind(py),
        };
        let bytes_from_fp: Vec<u8> = fp
            .call_method1(intern!(py, "read"), (&bytes_to_read,))?
            .extract()?;

        let num_read_bytes = bytes_from_fp.len();
        if num_read_bytes < minimum_amount {
            return raise_cbor_error(
                py,
                "CBORDecodeError",
                format!("premature end of stream (expected to read at least {minimum_amount} bytes, got {num_read_bytes} instead)").as_str(),
            );
        }
        self.buffer.extend(bytes_from_fp);
        Ok(())
    }

    fn read_exact<const N: usize>(slf: &Bound<'_, Self>) -> PyResult<[u8; N]> {
        let py = slf.py();
        let mut this = slf.borrow_mut();

        // If there's not enough data in the buffer, read some more
        let buffer_length = this.buffer.len();
        if N > buffer_length {
            this.read_to_buffer(py, N - buffer_length)?;
        }

        let mut output: [u8; N] = [0; N];
        output.copy_from_slice(this.buffer.drain(..N).as_slice());
        Ok(output)
    }

    fn set_shareable_internal<'py, T>(
        slf: &Bound<'_, Self>,
        value: Bound<'py, T>,
    ) -> Bound<'py, T> {
        let mut this = slf.borrow_mut();
        if let Some(index) = this.share_index {
            this.shareables[index] = value.clone().unbind().into_any()
        }
        value
    }

    fn create_incremental_utf8_decoder<'py>(
        py: Python<'py>,
        str_errors: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        py.import("codecs")?
            .getattr("lookup")?
            .call1(("utf-8",))?
            .getattr("incrementaldecoder")?
            .call1((str_errors,))
    }
}

#[pymethods]
impl CBORDecoder {
    #[new]
    #[pyo3(signature = (
        fp: "typing.IO[bytes]",
        *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        str_errors: "str" = "strict",
        read_size: "int" = 4096,
    ))]
    pub fn new(
        py: Python<'_>,
        fp: Option<&Bound<'_, PyAny>>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        str_errors: &str,
        read_size: u32,
    ) -> PyResult<Self> {
        let major_decoders: Bound<'_, PyDict> =
            py.import("cbor2")?.getattr("major_decoders")?.cast_into()?;
        let mut major_decoders_map: HashMap<u8, Py<PyAny>> = HashMap::new();
        for (key, value) in major_decoders.iter() {
            major_decoders_map.insert(key.extract::<u8>()?, value.clone().unbind());
        }

        let semantic_decoders: Bound<'_, PyDict> = py
            .import("cbor2")?
            .getattr("semantic_decoders")?
            .cast_into()?;
        let mut semantic_decoders_map: HashMap<u64, Py<PyAny>> = HashMap::new();
        for (key, value) in semantic_decoders.iter() {
            semantic_decoders_map.insert(key.extract::<u64>()?, value.clone().unbind());
        }

        let mut this = Self {
            fp: None,
            tag_hook: None,
            object_hook: None,
            str_errors: StrError::from_str(str_errors)?,
            read_size,
            major_decoders: major_decoders_map,
            semantic_decoders: semantic_decoders_map,
            buffer: Box::new(Vec::new()),
            decode_depth: 0,
            share_index: None,
            shareables: Vec::new(),
        };
        this.set_fp(fp)?;
        this.set_tag_hook(tag_hook)?;
        this.set_object_hook(object_hook)?;
        Ok(this)
    }

    #[getter]
    fn fp(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.fp.as_ref().map(|fp| fp.clone_ref(py))
    }

    #[setter]
    fn set_fp(&mut self, fp: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        if let Some(fp) = fp {
            let result = fp.getattr("read");
            if let Ok(read) = result
                && read.is_callable()
            {
                self.fp = Some(fp.clone().unbind());
            } else {
                return Err(PyValueError::new_err(
                    "fp must be a file-like object with a read() method",
                ));
            }
        } else {
            self.fp = None;
        }
        Ok(())
    }

    #[getter]
    fn tag_hook(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.tag_hook
            .as_ref()
            .map(|tag_hook| tag_hook.clone_ref(py))
    }

    #[setter]
    fn set_tag_hook(&mut self, tag_hook: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        if let Some(tag_hook) = tag_hook {
            if !tag_hook.is_callable() {
                return Err(PyErr::new::<PyTypeError, _>("tag_hook must be callable"));
            }

            self.tag_hook = Some(tag_hook.clone().unbind());
        } else {
            self.tag_hook = None;
        }
        Ok(())
    }

    #[getter]
    fn object_hook(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.object_hook
            .as_ref()
            .map(|object_hook| object_hook.clone_ref(py))
    }

    #[setter]
    fn set_object_hook(&mut self, object_hook: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        if let Some(object_hook) = object_hook {
            if !object_hook.is_callable() {
                return Err(PyErr::new::<PyTypeError, _>("object_hook must be callable"));
            }

            self.object_hook = Some(object_hook.clone().unbind());
        } else {
            self.object_hook = None;
        }
        Ok(())
    }

    fn read(slf: &Bound<'_, Self>, amount: usize) -> PyResult<Vec<u8>> {
        let py = slf.py();
        let mut this = slf.borrow_mut();

        // If there's not enough data in the buffer, read some more
        let buffer_length = this.buffer.len();
        if amount > buffer_length {
            this.read_to_buffer(py, amount - buffer_length)?;
        }

        Ok(this.buffer.drain(..amount).collect())
    }

    /// Set the shareable value for the last encountered shared value marker,
    /// if any. If the current shared index is ``None``, nothing is done.
    ///
    /// :param value: the shared value
    /// :returns: the shared value to permit chaining
    fn set_shareable<'py>(slf: &Bound<'_, Self>, value: Bound<'py, PyAny>) -> Bound<'py, PyAny> {
        let mut this = slf.borrow_mut();
        if let Some(index) = this.share_index {
            this.shareables[index] = value.clone().unbind().into_any()
        }
        value
    }

    /// Decode the next value from the stream.
    ///
    /// :raises CBORDecodeError: if there is any problem decoding the stream
    pub fn decode<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let initial_byte = Self::read_exact::<1>(slf)?[0];
        let major_type = initial_byte >> 5;
        let subtype = initial_byte & 31;
        let decoder = match slf.borrow().major_decoders.get(&major_type) {
            Some(decoder) => decoder.clone_ref(py).into_bound(py),
            None => {
                return raise_cbor_error(
                    py,
                    "CBORDecodeError",
                    format!("invalid major type: {major_type}").as_str(),
                );
            }
        };
        decoder.call1((subtype,))
    }

    fn decode_length_finite(slf: &Bound<'_, Self>, subtype: u8) -> PyResult<u64> {
        match Self::decode_length(slf, subtype)? {
            Some(length) => Ok(length),
            None => raise_cbor_error(
                slf.py(),
                "CBORDecodeValueError",
                "indefinite length not allowed here",
            )?,
        }
    }

    fn decode_length(slf: &Bound<'_, Self>, subtype: u8) -> PyResult<Option<u64>> {
        let length = match subtype {
            ..24 => Some(subtype as u64),
            24 => Some(Self::read_exact::<1>(slf)?[0] as u64),
            25 => Some(u16::from_be_bytes(Self::read_exact(slf)?) as u64),
            26 => Some(u32::from_be_bytes(Self::read_exact(slf)?) as u64),
            27 => Some(u64::from_be_bytes(Self::read_exact(slf)?)),
            31 => None,
            _ => {
                let msg = format!("unknown unsigned integer subtype 0x{subtype:x}");
                raise_cbor_error(slf.py(), "CBORDecodeValueError", msg.as_str())?
            }
        };
        Ok(length)
    }

    fn decode_uint<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 0
        let uint = Self::decode_length_finite(slf, subtype)?;
        let py_int = uint.into_bound_py_any(slf.py())?;
        Ok(Self::set_shareable(slf, py_int))
    }

    fn decode_negint<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 1
        let uint = Self::decode_length_finite(slf, subtype)?;
        let signed_int = -(uint as i128) - 1;
        let py_int = signed_int.into_bound_py_any(slf.py())?;
        Ok(Self::set_shareable(slf, py_int))
    }

    fn decode_bytestring<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyBytes>> {
        // Major tag 2
        let py = slf.py();
        let decoded = match Self::decode_length(slf, subtype)? {
            None => {
                // Indefinite length
                let mut output = PyBytes::new(py, b"");
                loop {
                    let obj = Self::decode(slf)?;
                    if let Ok(string) = obj.cast::<PyBytes>() {
                        output = output.add(string)?.cast_into()?;
                    } else if obj.is_exact_instance_of::<BreakMarkerType>() {
                        break output;
                    } else {
                        return raise_cbor_error(
                            py,
                            "CBORDecodeValueError",
                            "invalid major type in indefinite length bytestring",
                        );
                    }
                }
            }
            Some(length) if length <= 65536 => {
                let bytes = Self::read(slf, length as usize)?;
                PyBytes::new(py, &bytes)
            }
            Some(mut length) => {
                // Incrementally read the bytestring, in chunks of 65536 bytes
                let mut bytes = PyBytes::new(py, b"");
                while length > 0 {
                    let chunk_size = min(length, 65536) as usize;
                    let chunk = Self::read(slf, chunk_size)?;
                    length -= chunk_size as u64;
                    bytes = bytes.add(chunk)?.cast_into()?;
                }
                bytes
            }
        };
        Ok(Self::set_shareable_internal(slf, decoded))
    }

    fn decode_string<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyString>> {
        // Major tag 3
        let py = slf.py();
        let str_errors = &slf.borrow().str_errors;
        let decoded = match Self::decode_length(slf, subtype)? {
            None => {
                // Indefinite length
                let mut output = PyString::new(py, "");
                loop {
                    let obj = Self::decode(slf)?;
                    if let Ok(string) = obj.cast::<PyString>() {
                        output = output.add(string)?.cast_into()?;
                    } else if obj.is_exact_instance_of::<BreakMarkerType>() {
                        break output;
                    } else {
                        return raise_cbor_error(
                            py,
                            "CBORDecodeValueError",
                            "invalid major type in indefinite length string",
                        );
                    }
                }
            }
            Some(length) if length <= 65536 => {
                let bytes = Self::read(slf, length as usize)?;
                let py_bytes = bytes.into_bound_py_any(py)?;
                let decode_result =
                    py_bytes.call_method1(intern!(py, "decode"), ("utf-8", str_errors.as_str()));
                if let Ok(decoded_bytes) = decode_result {
                    decoded_bytes.cast_into().map_err(PyErr::from)?
                } else {
                    return raise_cbor_error_from(
                        py,
                        "CBORDecodeValueError",
                        "error decoding unicode string",
                        decode_result.unwrap_err(),
                    );
                }
            }
            Some(mut length) => {
                // Incrementally decode the string, in chunks of 65536 bytes
                let decoder = py
                    .import("codecs")?
                    .getattr("lookup")?
                    .call1(("utf-8",))?
                    .getattr("incrementaldecoder")?
                    .call1((str_errors.as_str(),))?;
                let mut string = PyString::new(py, "");
                while length > 0 {
                    let chunk_size = min(length, 65536) as usize;
                    let chunk = Self::read(slf, chunk_size)?;
                    length -= chunk_size as u64;
                    let is_final_chunk = length == 0;
                    let decode_result =
                        decoder.call_method1(intern!(py, "decode"), (chunk, is_final_chunk));
                    let decoded_chunk: Bound<'_, PyString> = match decode_result {
                        Ok(decoded_chunk) => decoded_chunk.cast_into()?,
                        Err(e) => {
                            return raise_cbor_error_from(
                                py,
                                "CBORDecodeValueError",
                                "error decoding unicode string",
                                e,
                            );
                        }
                    };
                    string = string.add(decoded_chunk)?.cast_into()?;
                }
                string
            }
        };
        Ok(Self::set_shareable_internal(slf, decoded))
    }

    ///  Wrap the given bytestring as a file and call :meth:`decode` with it as
    ///  the argument.
    ///
    ///  This method was intended to be used from the ``tag_hook`` hook when an
    ///  object needs to be decoded separately from the rest but while still
    ///  taking advantage of the shared value registry.
    #[pyo3(signature = (buf: "bytes"))]
    pub fn decode_from_bytes<'py>(
        slf: &Bound<'py, Self>,
        buf: Vec<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let old_fp = this.fp.as_ref().map(|fp| fp.clone_ref(py));
        let old_buffer = this.buffer.clone();
        this.fp = None;
        this.buffer = Box::new(buf);
        drop(this);
        let result = Self::decode(slf);
        this = slf.borrow_mut();
        this.fp = old_fp;
        this.buffer = old_buffer;
        result
    }
}
