use crate::types::{BreakMarkerType, CBORSimpleValue, CBORTag, FrozenDict};
use crate::utils::{create_cbor_error, raise_cbor_error, raise_cbor_error_from, wrap_cbor_error};
use half::f16;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyBytes, PyComplex, PyDict, PyFrozenSet, PyInt, PyList, PySet, PyString, PyTuple,
};
use pyo3::{IntoPyObjectExt, Py, PyAny, intern, pyclass};
use std::cmp::{max, min};
use std::collections::HashMap;
use std::mem::take;

const VALID_STR_ERRORS: [&str; 3] = ["strict", "ignore", "replace"];

#[pyclass(module = "cbor2")]
pub struct CBORDecoder {
    fp: Option<Py<PyAny>>,
    tag_hook: Option<Py<PyAny>>,
    object_hook: Option<Py<PyAny>>,
    str_errors: Py<PyAny>,
    read_size: u32,

    major_decoders: HashMap<u8, Py<PyAny>>,
    semantic_decoders: HashMap<u64, Py<PyAny>>,
    undefined: Py<PyAny>,
    break_marker: Py<PyAny>,
    buffer: Vec<u8>,

    decode_depth: u32,
    share_index: Option<usize>,
    shareables: Vec<Option<Py<PyAny>>>,
    stringref_namespace: Vec<Py<PyString>>,
    #[pyo3(get, set)]
    immutable: bool,
}

impl CBORDecoder {
    pub fn new_internal(
        py: Python<'_>,
        fp: Option<&Bound<'_, PyAny>>,
        buffer: Vec<u8>,
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

        let undefined = py.import("cbor2")?.getattr("undefined")?;
        let break_marker = py.import("cbor2")?.getattr("break_marker")?;

        let mut this = Self {
            fp: None,
            tag_hook: None,
            object_hook: None,
            str_errors: PyString::new(py, "strict").into_py_any(py)?,
            read_size,
            major_decoders: major_decoders_map,
            semantic_decoders: semantic_decoders_map,
            undefined: undefined.unbind(),
            break_marker: break_marker.unbind(),
            buffer,
            decode_depth: 0,
            share_index: None,
            shareables: Vec::new(),
            stringref_namespace: Vec::new(),
            immutable: false,
        };
        if let Some(fp) = fp {
            this.set_fp(fp)?
        };
        this.set_tag_hook(tag_hook)?;
        this.set_object_hook(object_hook)?;
        this.set_str_errors(py, str_errors)?;
        Ok(this)
    }

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
                "CBORDecodeEOF",
                format!(
                    "premature end of stream (expected to read at least {minimum_amount} \
                     bytes, got {num_read_bytes} instead)"
                )
                .as_str(),
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
            this.shareables[index] = Some(value.clone().unbind().into_any());
        }
        value
    }

    fn with_immutable<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> PyResult<T>) -> PyResult<T> {
        let mut this = slf.borrow_mut();
        let old_immutable = this.immutable;
        this.immutable = true;
        drop(this);

        let result = f();

        slf.borrow_mut().immutable = old_immutable;
        result
    }

    fn with_unshared<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> PyResult<T>) -> PyResult<T> {
        let mut this = slf.borrow_mut();
        let old_share_index = this.share_index;
        this.share_index = None;
        drop(this);

        let result = f();

        slf.borrow_mut().share_index = old_share_index;
        result
    }

    /// Increment the decoding depth, and if it drops to 0 after calling the given function,
    /// reset any value sharing state.
    fn decoding_context<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> T) -> T {
        slf.borrow_mut().decode_depth += 1;

        let result = f();

        let mut this = slf.borrow_mut();
        this.decode_depth -= 1;
        if this.decode_depth == 0 {
            this.shareables.clear();
            this.share_index = None;
        }
        result
    }
}

#[pymethods]
impl CBORDecoder {
    #[new]
    #[pyo3(signature = (
        fp: "typing.IO[bytes]",
        *,
        tag_hook: "collections.abc.Callable[[CBORDecoder, CBORTag], Any]] | None" = None,
        object_hook: "collections.abc.Callable[[CBORDecoder, dict[typing.Any, typing.Any], Any]]] | None" = None,
        str_errors: "str" = "strict",
        read_size: "int" = 1,
    ))]
    pub fn new(
        py: Python<'_>,
        fp: &Bound<'_, PyAny>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        str_errors: &str,
        read_size: u32,
    ) -> PyResult<Self> {
        Self::new_internal(
            py,
            Some(fp),
            Vec::new(),
            tag_hook,
            object_hook,
            str_errors,
            read_size,
        )
    }

    #[getter]
    fn fp(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.fp.as_ref().map(|fp| fp.clone_ref(py))
    }

    #[setter]
    fn set_fp(&mut self, fp: &Bound<'_, PyAny>) -> PyResult<()> {
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
                return Err(PyErr::new::<PyTypeError, _>(
                    "tag_hook must be callable or None",
                ));
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
                return Err(PyErr::new::<PyTypeError, _>(
                    "object_hook must be callable or None",
                ));
            }

            self.object_hook = Some(object_hook.clone().unbind());
        } else {
            self.object_hook = None;
        }
        Ok(())
    }

    #[getter]
    fn str_errors(&self, py: Python<'_>) -> PyResult<String> {
        self.str_errors.bind(py).extract()
    }

    #[setter]
    fn set_str_errors(&mut self, py: Python<'_>, str_errors: &str) -> PyResult<()> {
        if !VALID_STR_ERRORS.contains(&str_errors) {
            return Err(PyValueError::new_err(format!(
                "invalid str_errors value: '{str_errors}'"
            )));
        }
        self.str_errors = PyString::new(py, str_errors).into_py_any(py)?;
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
            this.shareables[index] = Some(value.clone().unbind().into_any())
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
        Self::decoding_context(slf, || decoder.call1((slf, subtype)))
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
        let old_buffer = take(&mut this.buffer);
        this.fp = None;
        this.buffer = buf;
        drop(this);

        let result = Self::decode(slf);

        this = slf.borrow_mut();
        this.fp = old_fp;
        this.buffer = old_buffer;
        result
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

    //
    // Decoders for major tags (0-7)
    //

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

    #[pyo3(signature = (subtype: "int"))]
    fn decode_uint<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 0
        let uint = Self::decode_length_finite(slf, subtype)?;
        let py_int = uint.into_bound_py_any(slf.py())?;
        Ok(Self::set_shareable(slf, py_int))
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_negint<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 1
        let uint = Self::decode_length_finite(slf, subtype)?;
        let signed_int = -(uint as i128) - 1;
        let py_int = signed_int.into_bound_py_any(slf.py())?;
        Ok(Self::set_shareable(slf, py_int))
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_bytestring<'py>(
        slf: &Bound<'py, Self>,
        subtype: u8,
    ) -> PyResult<Bound<'py, PyBytes>> {
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

    #[pyo3(signature = (subtype: "int"))]
    fn decode_string<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyString>> {
        // Major tag 3
        let py = slf.py();
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
                let str_errors = &slf.borrow().str_errors.clone_ref(py);
                let decode_result =
                    py_bytes.call_method1(intern!(py, "decode"), ("utf-8", str_errors));
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
                let this = slf.borrow();
                let decoder = py
                    .import("codecs")?
                    .getattr("lookup")?
                    .call1(("utf-8",))?
                    .getattr("incrementaldecoder")?
                    .call1((this.str_errors.bind(py),))?;
                drop(this);
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

    #[pyo3(signature = (subtype: "int"))]
    fn decode_array<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 4
        let py = slf.py();
        let immutable = slf.borrow().immutable;
        match Self::decode_length(slf, subtype)? {
            None => {
                // Indefinite length
                let mut items = Vec::<Bound<'_, PyAny>>::new();
                if immutable {
                    // Construct a tuple (not shareable)
                    loop {
                        let obj = Self::decode(slf)?;
                        if obj.is_exact_instance_of::<BreakMarkerType>() {
                            let tuple = PyTuple::new(py, items)?;
                            break Ok(tuple.into_any());
                        }
                        items.push(obj);
                    }
                } else {
                    // Construct a list (shareable)
                    let list = Self::set_shareable_internal(slf, PyList::empty(py));
                    loop {
                        let obj = Self::decode(slf)?;
                        if obj.is_exact_instance_of::<BreakMarkerType>() {
                            break Ok(list.into_any());
                        } else {
                            list.append(obj)?;
                        }
                    }
                }
            }
            Some(length) => {
                let mut items = Vec::<Bound<'_, PyAny>>::with_capacity(length as usize);
                for _ in 0..length {
                    items.push(Self::decode(slf)?);
                }

                match immutable {
                    true => Ok(PyTuple::new(py, items)?.into_any()),
                    false => Ok(PyList::new(py, items)?.into_any()),
                }
            }
        }
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_map<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 5
        let py = slf.py();
        let mut dict = Self::set_shareable_internal(slf, PyDict::new(py));
        match Self::decode_length(slf, subtype)? {
            None => {
                // Indefinite length
                loop {
                    let key = Self::decode(slf)?;
                    if key.is_exact_instance_of::<BreakMarkerType>() {
                        break;
                    }
                    let value = Self::decode(slf)?;
                    dict.set_item(key, value)?;
                }
            }
            Some(length) => {
                for _ in 0..length {
                    let key = Self::decode(slf)?;
                    let value = Self::decode(slf)?;
                    dict.set_item(key, value)?;
                }
            }
        }

        // If an object hook was specified, call it now with the constructed dictionary and use its
        // return value as the final dictionary
        if let Some(object_hook) = &slf.borrow().object_hook {
            match object_hook.bind_borrowed(py).call1((&slf, &dict)) {
                Ok(retval) => {
                    return Ok(retval);
                }
                Err(e) => {
                    raise_cbor_error_from(py, "CBORDecodeError", "error calling object hook", e)?;
                }
            }
        }

        // If we're constructing an immutable map, wrap the dict in a FrozenDict
        if slf.borrow().immutable {
            let args = PyTuple::new(py, (&dict,).into_pyobject(py))?;
            FrozenDict::new(&args)?.into_bound_py_any(py)
        } else {
            Ok(dict.into_any())
        }
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_semantic<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let tagnum = Self::decode_length_finite(slf, subtype)?;
        let this = slf.borrow();
        match this
            .semantic_decoders
            .get(&tagnum)
            .map(|decoder| decoder.clone_ref(py))
        {
            Some(decoder) => {
                drop(this);
                slf.borrow_mut();
                decoder.bind(py).call1((slf,))
            }
            None => {
                drop(this);
                let value = Self::decode(slf)?;
                CBORTag::new(tagnum.into_bound_py_any(py)?, value)?.into_bound_py_any(py)
            }
        }
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_special<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 7
        let py = slf.py();
        match subtype {
            0..20 => {
                let value = subtype.into_pyobject(py)?;
                CBORSimpleValue::new(value)?.into_bound_py_any(py)
            }
            20 => Ok(false.into_bound_py_any(py)?),
            21 => Ok(true.into_bound_py_any(py)?),
            22 => Ok(py.None().into_bound_py_any(py)?),
            23 => Ok(slf.borrow().undefined.clone_ref(py).into_bound(py)),
            24 => {
                let value = Self::read_exact::<1>(slf)?[0];
                CBORSimpleValue::new(value.into_pyobject(py)?)?.into_bound_py_any(py)
            }
            25 => {
                let bytes = Self::read_exact::<2>(slf)?;
                f16::from_be_bytes(bytes).to_f32().into_bound_py_any(py)
            }
            26 => {
                let bytes = Self::read_exact::<4>(slf)?;
                f32::from_be_bytes(bytes).into_bound_py_any(py)
            }
            27 => {
                let bytes = Self::read_exact::<8>(slf)?;
                f64::from_be_bytes(bytes).into_bound_py_any(py)
            }
            31 => Ok(slf.borrow().break_marker.clone_ref(py).into_bound(py)),
            _ => {
                let msg = format!("undefined reserved major type 7 subtype 0x{subtype:x}");
                raise_cbor_error(py, "CBORDecodeValueError", msg.as_str())
            }
        }
    }

    //
    // Decoders for semantic tags (major tag 6)
    //

    fn decode_epoch_date<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 100
        let value = Self::decode(slf)?.extract::<i32>()? + 719163;
        let date_class = slf.py().import("datetime")?.getattr("date")?;
        let date = date_class.call_method1("fromordinal", (value,))?;
        Ok(Self::set_shareable(slf, date))
    }

    fn decode_shareable<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 28
        let mut this = slf.borrow_mut();
        let old_index = this.share_index;
        this.share_index = Some(this.shareables.len());
        this.shareables.push(None);
        drop(this);

        let result = Self::decode(slf);

        slf.borrow_mut().share_index = old_index;
        result
    }

    fn decode_sharedref<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 29
        let py = slf.py();
        let index: usize = Self::with_unshared(slf, || Self::decode(slf))?.extract()?;
        match slf.borrow().shareables.get(index) {
            None => raise_cbor_error(
                slf.py(),
                "CBORDecodeValueError",
                format!("shared reference {index} not found").as_str(),
            ),
            Some(None) => raise_cbor_error(
                slf.py(),
                "CBORDecodeValueError",
                format!("shared value {index} has not been initialized").as_str(),
            ),
            Some(Some(shared)) => Ok(shared.clone_ref(py).into_bound(py)),
        }
    }

    fn decode_date_string<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 1004
        let value = Self::decode(slf)?;
        let date_class = slf.py().import("datetime")?.getattr("date")?;
        let date = date_class.call_method1("fromisoformat", (value,))?;
        Ok(Self::set_shareable(slf, date))
    }

    fn decode_datetime_string<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 0
        let py = slf.py();
        let value = Self::decode(slf)?;
        let value_type = value.get_type();
        let mut datetime_str: Bound<PyString> = value.cast_into().map_err(|e| {
            create_cbor_error(
                py,
                "CBORDecodeValueError",
                format!(
                    "expected string for tag, got {} instead",
                    value_type.to_string()
                )
                .as_str(),
                Some(PyErr::from(e)),
            )
        })?;

        // Python 3.10 has impaired parsing of the ISO format:
        // * It doesn't handle the standard "Z" suffix
        // * It doesn't handle the fractional seconds part having fewer than 6 digits
        if py.version_info() <= (3, 10) {
            // Convert Z to +00:00
            let mut temp_str = datetime_str.to_string().replacen("Z", "+00:00", 1);

            // Pad any microseconds part with zeros
            if let Some((first, second)) = temp_str.split_once('.') {
                if let Some(index) = second.find(|c: char| !c.is_numeric()) {
                    let (mut micros, tz_part) = second.split_at(index);
                    // Cut off excess zeroes from the start of the microseconds part
                    if micros.len() >= 6 {
                        micros = &micros[..6];
                    }

                    // Reconstitute the datetime string, right-padding the microseconds part
                    // with zeroes
                    temp_str = format!("{first}.{micros:0<6}{tz_part}");
                }
            }

            datetime_str = temp_str.into_pyobject(py)?;
        }

        let datetime_class = slf.py().import("datetime")?.getattr("datetime")?;
        let datetime = datetime_class
            .call_method1("fromisoformat", (&datetime_str,))
            .map_err(|e| {
                create_cbor_error(
                    py,
                    "CBORDecodeValueError",
                    format!("invalid datetime string: '{datetime_str}'").as_str(),
                    Some(e),
                )
            })?;
        Ok(Self::set_shareable(slf, datetime))
    }

    fn decode_epoch_datetime<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 1
        let value = Self::decode(slf)?;
        let datetime_class = slf.py().import("datetime")?.getattr("datetime")?;
        let utc = slf
            .py()
            .import("datetime")?
            .getattr("timezone")?
            .getattr("utc")?;
        datetime_class
            .call_method1("fromtimestamp", (value, utc))
            .map_err(|e| {
                create_cbor_error(
                    slf.py(),
                    "CBORDecodeValueError",
                    "error decoding datetime from epoch",
                    Some(e),
                )
            })
    }

    fn decode_positive_bignum<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 2
        let py = slf.py();
        let int_type = py.get_type::<PyInt>();
        let value = Self::decode(slf)?;
        let int = int_type.call_method1("from_bytes", (value, intern!(py, "big")))?;
        Ok(Self::set_shareable(slf, int))
    }

    fn decode_negative_bignum<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 3
        let py = slf.py();
        let int_type = py.get_type::<PyInt>();
        let value = Self::decode(slf)?;
        let mut int = int_type.call_method1("from_bytes", (value, intern!(py, "big")))?;
        int = int.neg()?.add(-1)?;
        Ok(Self::set_shareable_internal(slf, int))
    }

    fn decode_fraction<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 4
        let py = slf.py();
        let decimal_class = py.import("decimal")?.getattr("Decimal")?;
        let value = Self::with_immutable(slf, || Self::decode(slf))?;
        let tuple = value.cast::<PyTuple>().map_err(|e| {
            create_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding decimal fraction: input value must be an array",
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding decimal fraction: array must have exactly two elements",
            );
        }

        let decimal =
            wrap_cbor_error(py, "CBORDecodeValueError", "error decoding decimal fraction", || {
                let exp = tuple.get_item(0)?;
                let sig_tuple = decimal_class.call1((tuple.get_item(1)?,))?.call_method0("as_tuple")?.cast_into::<PyTuple>()?;
                let sign = sig_tuple.get_item(0)?;
                let digits = sig_tuple.get_item(1)?;
                let args_tuple = PyTuple::new(py, [sign, digits, exp])?;
                decimal_class.call1((args_tuple,))
            })?;
        Ok(Self::set_shareable(slf, decimal))
    }

    fn decode_bigfloat<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 5
        let py = slf.py();
        let decimal_class = py.import("decimal")?.getattr("Decimal")?;
        let value = Self::with_immutable(slf, || Self::decode(slf))?;
        let tuple = value.cast::<PyTuple>().map_err(|e| {
            create_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding bigfloat: input value must be an array",
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding bigfloat: array must have exactly two elements",
            );
        }

        let decimal =
            wrap_cbor_error(py, "CBORDecodeValueError", "error decoding bigfloat", || {
                let exp = decimal_class.call1((tuple.get_item(0)?,))?;
                let sig = decimal_class.call1((tuple.get_item(1)?,))?;
                let exp = PyInt::new(py, 2).pow(exp, py.None())?;
                sig.mul(exp)
            })?;
        Ok(Self::set_shareable(slf, decimal))
    }

    fn decode_rational<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 30
        let py = slf.py();
        let fraction_class = py.import("fractions")?.getattr("Fraction")?;
        let value = Self::with_immutable(slf, || Self::decode(slf))?;
        let tuple = value.cast_into::<PyTuple>().map_err(|e| {
            create_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding rational: input value must be an array",
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding rational: array must have exactly two elements",
            );
        }

        match fraction_class.call1(tuple) {
            Ok(fraction) => Ok(Self::set_shareable(slf, fraction)),
            Err(e) => {
                raise_cbor_error_from(py, "CBORDecodeValueError", "error decoding rational", e)
            }
        }
    }

    fn decode_regexp<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 35
        let py = slf.py();
        let value = Self::decode(slf)?;
        let re_compile_func = py.import("re")?.getattr("compile")?;
        match re_compile_func.call1((value,)) {
            Ok(regexp) => Ok(Self::set_shareable(slf, regexp)),
            Err(e) => raise_cbor_error_from(
                py,
                "CBORDecodeValueError",
                "error decoding regular expression",
                e,
            ),
        }
    }

    fn decode_mime<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 36
        let value = Self::decode(slf)?;
        let email_parser_class = slf.py().import("email.parser")?.getattr("Parser")?;
        let parser = email_parser_class.call0()?;
        match parser.call_method1("parsestr", (value,)) {
            Ok(message) => Ok(Self::set_shareable(slf, message)),
            Err(e) => raise_cbor_error_from(
                slf.py(),
                "CBORDecodeValueError",
                "error decoding MIME message",
                e,
            ),
        }
    }

    fn decode_uuid<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 37
        let py = slf.py();
        let value = Self::decode(slf)?;
        let uuid_class = py.import("uuid")?.getattr("UUID")?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("bytes", value)?;
        match uuid_class.call((), Some(&kwargs)) {
            Ok(uuid) => Ok(Self::set_shareable(slf, uuid)),
            Err(e) => raise_cbor_error_from(
                py,
                "CBORDecodeValueError",
                "error decoding UUID value",
                e,
            ),
        }
    }

    fn decode_stringref_namespace<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 256
        let mut this = slf.borrow_mut();
        let old_namespace = take(&mut this.stringref_namespace);
        this.stringref_namespace = Vec::new();
        drop(this);
        let value = Self::decode(slf)?;
        slf.borrow_mut().stringref_namespace = old_namespace;
        Ok(value)
    }

    fn decode_set<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 258
        let py = slf.py();
        let tuple: Bound<'_, PyTuple> =
            Self::with_immutable(slf, || Self::decode(slf))?.cast_into()?;
        let set = if slf.borrow().immutable {
            PyFrozenSet::new(py, tuple.iter())?.into_any()
        } else {
            PySet::new(py, tuple.iter())?.into_any()
        };
        Ok(Self::set_shareable(slf, set))
    }

    fn decode_complex<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyComplex>> {
        // Semantic tag 43000
        let py = slf.py();
        let value = Self::with_immutable(slf, || Self::decode(slf))?;
        let tuple = value.cast_into::<PyTuple>().map_err(|e| {
            create_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding complex: input value must be an array",
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding complex: array must have exactly two elements",
            );
        }

        wrap_cbor_error(py, "CBORDecodeValueError", "error decoding complex", || {
            let real: f64 = tuple.get_item(0)?.extract()?;
            let imag: f64 = tuple.get_item(1)?.extract()?;
            Ok(PyComplex::from_doubles(py, real, imag))
        })
    }
}
