use crate::_cbor2::SYS_MAXSIZE;
use crate::_cbor2::{BREAK_MARKER, UNDEFINED};
use crate::_cbor2::{DEFAULT_MAX_DEPTH, DEFAULT_READ_SIZE};
use crate::types::{BreakMarkerType, CBORSimpleValue, CBORTag, FrozenDict, DECIMAL_TYPE, FRACTION_TYPE, IPV4ADDRESS_TYPE, IPV4INTERFACE_TYPE, IPV4NETWORK_TYPE, IPV6ADDRESS_TYPE, IPV6INTERFACE_TYPE, IPV6NETWORK_TYPE, UUID_TYPE};
use crate::utils::{CBORDecodeError, create_cbor_error, raise_cbor_error, raise_cbor_error_from, wrap_cbor_error, PyImportable};
use half::f16;
use pyo3::exceptions::{PyException, PyLookupError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyComplex, PyDict, PyFrozenSet, PyInt, PyList, PyMapping, PySet, PyString, PyTuple};
use pyo3::{IntoPyObjectExt, Py, PyAny, intern, pyclass};
use std::cmp::{max, min};
use std::mem::{replace, take};

const VALID_STR_ERRORS: [&str; 3] = ["strict", "ignore", "replace"];
const SEEK_CUR: u8 = 1;


static DATE_FROMISOFORMAT: PyImportable = PyImportable::new("datetime", "date.fromisoformat");
static DATE_FROMORDINAL: PyImportable = PyImportable::new("datetime", "date.fromordinal");
static DATETIME_FROMISOFORMAT: PyImportable = PyImportable::new("datetime", "datetime.fromisoformat");
static DATETIME_FROMTIMESTAMP: PyImportable = PyImportable::new("datetime", "datetime.fromtimestamp");
static EMAIL_PARSER: PyImportable = PyImportable::new("email.parser", "Parser");
static INT_FROMBYTES: PyImportable = PyImportable::new("builtins", "int.from_bytes");
static IPADDRESS_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_address");
static IPNETWORK_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_network");
static IPINTERFACE_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_interface");
static RE_COMPILE: PyImportable = PyImportable::new("re", "compile");
static UTC: PyImportable = PyImportable::new("datetime", "timezone.utc");

/// The CBORDecoder class implements a fully featured `CBOR`_ decoder with
/// several extensions for handling shared references, big integers, rational
/// numbers and so on. Typically, the class is not used directly, but the
/// :func:`load` and :func:`loads` functions are called to indirectly construct
/// and use the class.
///
/// When the class is constructed manually, the main entry points are
/// :meth:`decode` and :meth:`decode_from_bytes`.
///
/// :param bytes data:
///     the bytestring to deserialize
/// :param tag_hook:
///     callable that takes 2 arguments: the decoder instance, and the :class:`.CBORTag`
///     to be decoded. This callback is invoked for any tags for which there is no
///     built-in decoder. The return value is substituted for the :class:`.CBORTag`
///     object in the deserialized output
/// :param object_hook:
///     callable that takes 2 arguments: the decoder instance, and a dictionary. This
///     callback is invoked for each deserialized :class:`dict` object. The return value
///     is substituted for the dict in the deserialized output.
/// :param major_decoders:
///     An optional mapping for overriding the decoders for select major types.
///     The value is a mapping of major types (integers 0-7) to callable that take 2
///     arguments: the decoder instance and a numeric subtype.
/// :param semantic_decoders:
///     An optional mapping for overriding the decoding for select semantic tags.
///     The value is a mapping of semantic tags (integers) to callables that take
///     the decoder instance as the sole argument.
/// :param str_errors:
///     determines how to handle unicode decoding errors (see the `Error Handlers`_
///     section in the standard library documentation for details)
/// :param int read_size: minimum amount of bytes to read at once (if ``fp`` is seekable)
/// :param int max_depth:
///     maximum allowed depth for nested containers
///
/// .. _CBOR: https://cbor.io/
#[pyclass(module = "cbor2")]
pub struct CBORDecoder {
    fp: Option<Py<PyAny>>,
    tag_hook: Option<Py<PyAny>>,
    object_hook: Option<Py<PyAny>>,
    major_decoders: Option<Py<PyMapping>>,
    semantic_decoders: Option<Py<PyMapping>>,
    str_errors: Py<PyString>,
    #[pyo3(get)]
    read_size: usize,
    #[pyo3(get)]
    max_depth: usize,

    read_method: Option<Py<PyAny>>,
    buffer: Option<Py<PyBytes>>,
    read_position: usize,
    available_bytes: usize,
    decode_depth: usize,
    fp_is_seekable: bool,
    share_index: Option<usize>,
    shareables: Vec<Option<Py<PyAny>>>,
    stringref_namespace: Option<Vec<Py<PyAny>>>,
    #[pyo3(get, set)]
    immutable: bool,
}

impl CBORDecoder {
    pub fn new_internal(
        py: Python<'_>,
        fp: Option<&Bound<'_, PyAny>>,
        buffer: Option<Bound<PyBytes>>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        major_decoders: Option<&Bound<'_, PyMapping>>,
        semantic_decoders: Option<&Bound<'_, PyMapping>>,
        str_errors: &str,
        read_size: usize,
        max_depth: usize,
    ) -> PyResult<Self> {
        let available_bytes = if let Some(buffer) = buffer.as_ref() {
            buffer.len()?
        } else {
            0
        };
        let bound_str_errors = PyString::new(py, str_errors);
        let mut this = Self {
            fp: None,
            tag_hook: None,
            object_hook: None,
            str_errors: bound_str_errors.clone().unbind(),
            read_size,
            max_depth,
            major_decoders: major_decoders.map(|d| d.clone().unbind()),
            semantic_decoders: semantic_decoders.map(|d| d.clone().unbind()),
            read_method: None,
            buffer: buffer.map(Bound::unbind),
            read_position: 0,
            available_bytes,
            decode_depth: 0,
            fp_is_seekable: false,
            share_index: None,
            shareables: Vec::new(),
            stringref_namespace: None,
            immutable: false,
        };
        if let Some(fp) = fp {
            this.set_fp(fp)?
        };
        this.set_tag_hook(tag_hook)?;
        this.set_object_hook(object_hook)?;
        this.set_str_errors(&bound_str_errors)?;
        Ok(this)
    }

    fn read_from_fp<'py>(
        &mut self,
        py: Python<'py>,
        minimum_amount: usize,
    ) -> PyResult<(Bound<'py, PyBytes>, usize)> {
        let read_size: usize = if self.fp_is_seekable {
            self.read_size
        } else {
            1
        };
        let bytes_to_read = max(minimum_amount, read_size);
        let num_read_bytes = if let Some(read) = self.read_method.as_ref() {
            let bytes_from_fp: Bound<PyBytes> =
                read.bind(py).call1((&bytes_to_read,))?.cast_into()?;
            let num_read_bytes = bytes_from_fp.len()?;
            if num_read_bytes >= minimum_amount {
                // self.buffer = Some(bytes_from_fp.unbind());
                return Ok((bytes_from_fp, num_read_bytes));
            }
            num_read_bytes
        } else {
            0
        };
        raise_cbor_error(
            py,
            "CBORDecodeEOF",
            format!(
                "premature end of stream (expected to read at least {minimum_amount} \
                 bytes, got {num_read_bytes} instead)"
            )
            .as_str(),
        )
    }

    fn read_exact<const N: usize>(&mut self, py: Python<'_>) -> PyResult<[u8; N]> {
        if self.available_bytes == 0 {
            // No buffer
            let (new_bytes, amount_read) = self.read_from_fp(py, N)?;
            self.read_position = N;
            self.available_bytes = amount_read - N;
            self.buffer = Some(new_bytes.unbind());
            Ok(self.buffer.as_ref().unwrap().as_bytes(py)[..N].try_into()?)
        } else if self.available_bytes < N {
            // Combine the remnants of the partial buffer with new data read from the file
            let needed_bytes = N - self.available_bytes;
            let mut concatenated_buffer: Vec<u8> = self.buffer.take().unwrap().extract(py)?;
            let (new_bytes, amount_read) = self.read_from_fp(py, needed_bytes)?;
            concatenated_buffer.extend_from_slice(&new_bytes[..needed_bytes]);
            self.buffer = Some(new_bytes.unbind());
            self.available_bytes = amount_read - needed_bytes;
            self.read_position = needed_bytes;
            Ok(concatenated_buffer.try_into().unwrap())
        } else {
            // Return a slice from the existing bytes object
            let slice: [u8; N] = self.buffer.as_ref().unwrap().bind(py).as_bytes()
                [self.read_position..self.read_position + N]
                .try_into()?;
            self.available_bytes -= N;
            self.read_position += N;
            Ok(slice)
        }
    }

    fn read_major_and_subtype(&mut self, py: Python<'_>) -> PyResult<(u8, u8)> {
        let initial_byte = self.read_exact::<1>(py)?[0];
        let major_type = initial_byte >> 5;
        let subtype = initial_byte & 31;
        Ok((major_type, subtype))
    }

    fn decode_length_finite(&mut self, py: Python<'_>, subtype: u8) -> PyResult<usize> {
        match self.decode_length(py, subtype)? {
            Some(length) => Ok(length),
            None => raise_cbor_error(
                py,
                "CBORDecodeValueError",
                "indefinite length not allowed here",
            )?,
        }
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

    fn add_string_to_namespace(&mut self, string: &Bound<PyAny>, length: usize) {
        // `string` must be either a PyString or PyBytes object
        if let Some(stringref_namespace) = self.stringref_namespace.as_mut() {
            let is_referenced = match stringref_namespace.len() {
                0..24 => length >= 3,
                24..256 => length >= 4,
                256..65536 => length >= 5,
                65536..4294967296 => length >= 6,
                _ => length >= 11,
            };
            if is_referenced {
                stringref_namespace.push(string.clone().into_any().unbind());
            }
        }
    }
}

#[pymethods]
impl CBORDecoder {
    #[new]
    #[pyo3(signature = (
        fp: "typing.IO[bytes]",
        *,
        tag_hook: "collections.abc.Callable[[CBORDecoder, CBORTag], typing.Any]] | None" = None,
        object_hook: "collections.abc.Callable[[CBORDecoder, dict[typing.Any, typing.Any], typing.Any]]] | None" = None,
        major_decoders = None,
        semantic_decoders = None,
        str_errors: "str" = "strict",
        read_size: "int" = DEFAULT_READ_SIZE,
        max_depth: "int" = DEFAULT_MAX_DEPTH,
    ))]
    pub fn new(
        py: Python<'_>,
        fp: &Bound<'_, PyAny>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        major_decoders: Option<&Bound<'_, PyMapping>>,
        semantic_decoders: Option<&Bound<'_, PyMapping>>,
        str_errors: &str,
        read_size: usize,
        max_depth: usize,
    ) -> PyResult<Self> {
        Self::new_internal(
            py,
            Some(fp),
            None,
            tag_hook,
            object_hook,
            major_decoders,
            semantic_decoders,
            str_errors,
            read_size,
            max_depth,
        )
    }

    #[getter]
    fn fp(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.fp.as_ref().map(|fp| fp.clone_ref(py))
    }

    #[setter]
    fn set_fp(&mut self, fp: &Bound<'_, PyAny>) -> PyResult<()> {
        let result = fp.call_method0("readable");
        if let Ok(readable) = &result
            && readable.is_truthy()?
        {
            self.fp_is_seekable = fp.call_method0("seekable")?.is_truthy()?;
            let fp = fp.clone();
            self.read_method = Some(fp.getattr("read")?.unbind());
            self.fp = Some(fp.unbind());
            self.available_bytes = 0;
            self.read_position = 0;
            self.buffer = None;
            Ok(())
        } else {
            let exc = PyValueError::new_err("fp must be a readable file-like object");
            exc.set_cause(fp.py(), result.err());
            Err(exc)
        }
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
    fn set_str_errors(&mut self, str_errors: &Bound<'_, PyString>) -> PyResult<()> {
        let as_string: &str = str_errors.extract()?;
        if !VALID_STR_ERRORS.contains(&as_string) {
            return Err(PyValueError::new_err(format!(
                "invalid str_errors value: '{str_errors}'"
            )));
        }
        self.str_errors = str_errors.clone().unbind();
        Ok(())
    }

    /// Read bytes from the data stream.
    ///
    /// :param int amount: the number of bytes to read
    /// :rtype: bytes
    #[pyo3(signature = (amount: "int", /))]
    fn read(&mut self, py: Python<'_>, amount: usize) -> PyResult<Vec<u8>> {
        if amount == 0 {
            return Ok(Vec::default());
        }

        if self.available_bytes == 0 {
            // No buffer
            let (new_bytes, amount_read) = self.read_from_fp(py, amount)?;
            self.read_position = amount;
            self.available_bytes = amount_read - amount;
            let new_buffer = new_bytes.as_bytes()[..amount].to_vec();
            self.buffer = Some(new_bytes.unbind());
            Ok(new_buffer)
        } else if self.available_bytes < amount {
            // Combine the remnants of the partial buffer with new data read from the file
            let needed_bytes = amount - self.available_bytes;
            let mut concatenated_buffer: Vec<u8> =
                self.buffer.take().unwrap().as_bytes(py).to_vec();
            let (new_bytes, amount_read) = self.read_from_fp(py, needed_bytes)?;
            concatenated_buffer.extend_from_slice(&new_bytes[..needed_bytes]);
            self.buffer = Some(new_bytes.unbind());
            self.available_bytes = amount_read - needed_bytes;
            self.read_position = needed_bytes;
            Ok(concatenated_buffer)
        } else {
            // Return a slice from the existing bytes object
            let vec = self.buffer.as_ref().unwrap().as_bytes(py)
                [self.read_position..self.read_position + amount]
                .to_vec();
            self.available_bytes -= amount;
            self.read_position += amount;
            Ok(vec)
        }
    }

    /// Set the shareable value for the last encountered shared value marker,
    /// if any. If the current shared index is :data:`None`, nothing is done.
    ///
    /// :param value: the shared value
    fn set_shareable<'py>(&mut self, value: &Bound<'py, PyAny>) -> Bound<'py, PyAny> {
        if let Some(index) = self.share_index {
            self.shareables[index] = Some(value.clone().unbind().into_any());
            self.share_index = None;
        }
        value.clone()
    }

    /// Decode the next value from the stream.
    ///
    /// :raises CBORDecodeError: if there is any problem decoding the stream
    pub fn decode<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let (major_type, subtype) = this.read_major_and_subtype(py)?;

        if this.decode_depth == this.max_depth {
            return raise_cbor_error(
                py,
                "CBORDecodeError",
                format!(
                    "maximum container nesting depth ({}) exceeded",
                    this.max_depth
                )
                .as_str(),
            );
        }

        if let Some(major_decoders) = &this.major_decoders {
            match major_decoders.bind(py).get_item(&major_type) {
                Ok(decoder) => {
                    this.decode_depth += 1;
                    drop(this);
                    let result = decoder.call1((slf, subtype));
                    slf.borrow_mut().decode_depth -= 1;
                    return result
                },
                Err(e) if e.is_instance_of::<PyLookupError>(py) => {}
                Err(e) => return Err(e)
            }
        }

        this.decode_depth += 1;
        let result = match major_type {
            0 => this.decode_uint(py, subtype),
            1 => this.decode_negint(py, subtype),
            2 => this.decode_bytestring(py, subtype)?.into_bound_py_any(py),
            3 => this.decode_string(py, subtype)?.into_bound_py_any(py),
            4 => {
                drop(this);
                let result = Self::decode_array(slf, subtype);
                this = slf.borrow_mut();
                result
            }
            5 => {
                drop(this);
                let result = Self::decode_map(slf, subtype);
                this = slf.borrow_mut();
                result
            }
            6 => {
                drop(this);
                let result = Self::decode_semantic(slf, subtype);
                this = slf.borrow_mut();
                result
            }
            7 => this.decode_special(py, subtype),
            _ => {
                return raise_cbor_error(
                    py,
                    "CBORDecodeError",
                    format!("invalid major type: {major_type}").as_str(),
                );
            }
        };
        this.decode_depth -= 1;

        // Clear shareables and string references to prevent any leaks
        if this.decode_depth == 0 {
            this.shareables.clear();
            this.stringref_namespace = None;
            this.share_index = None;

            // If fp was seekable and excess data has been read, empty the buffer and rewind the
            // file
            if this.available_bytes > 0
                && let Some(fp) = &this.fp
            {
                let offset = -(this.available_bytes as isize);
                fp.call_method1(py, intern!(py, "seek"), (offset, SEEK_CUR))?;
                this.buffer = None;
                this.available_bytes = 0;
                this.read_position = 0;
            }
        }

        result.map_err(|err| {
            if err.is_instance_of::<CBORDecodeError>(py) {
                err
            } else if err.is_instance_of::<PyValueError>(py) {
                create_cbor_error(
                    py,
                    "CBORDecodeValueError",
                    err.to_string().as_str(),
                    Some(err),
                )
            } else if err.is_instance_of::<PyException>(py) {
                create_cbor_error(py, "CBORDecodeError", err.to_string().as_str(), Some(err))
            } else {
                err
            }
        })
    }

    /// Wrap the given bytestring as a file and call :meth:`decode` with it as
    /// the argument.
    ///
    /// This method was intended to be used from the ``tag_hook`` hook when an
    /// object needs to be decoded separately from the rest but while still
    /// taking advantage of the shared value registry.
    ///
    /// :param bytes buf: the buffer from which to decode a CBOR object
    #[pyo3(signature = (buf: "bytes", /))]
    pub fn decode_from_bytes<'py>(
        slf: &Bound<'py, Self>,
        buf: Bound<'py, PyBytes>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut this = slf.borrow_mut();
        let fp = this.fp.take();
        let read_position = replace(&mut this.read_position, 0);
        let available_bytes = replace(&mut this.available_bytes, buf.len()?);
        let buffer = replace(&mut this.buffer, Some(buf.unbind()));
        drop(this);

        let result = Self::decode(slf);

        this = slf.borrow_mut();
        this.fp = fp;
        this.buffer = buffer;
        this.read_position = read_position;
        this.available_bytes = available_bytes;
        result
    }

    //
    // Decoders for major tags (0-7)
    //

    /// Decode the length of the next item.
    ///
    /// This is a low-level operation that may be needed by custom decoder callbacks.
    ///
    /// :param int subtype:
    /// :return: the length of the item, or :data:`None` to indicate an indefinite-length item
    /// :rtype: int | None
    fn decode_length(&mut self, py: Python<'_>, subtype: u8) -> PyResult<Option<usize>> {
        let length = match subtype {
            ..24 => Some(subtype as usize),
            24 => Some(self.read_exact::<1>(py)?[0] as usize),
            25 => Some(u16::from_be_bytes(self.read_exact(py)?) as usize),
            26 => Some(u32::from_be_bytes(self.read_exact(py)?) as usize),
            27 => Some(u64::from_be_bytes(self.read_exact(py)?) as usize),
            31 => None,
            _ => {
                let msg = format!("unknown unsigned integer subtype 0x{subtype:x}");
                raise_cbor_error(py, "CBORDecodeValueError", msg.as_str())?
            }
        };
        Ok(length)
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_uint<'py>(&mut self, py: Python<'py>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 0
        let uint = self.decode_length_finite(py, subtype)?;
        let py_int = uint.into_bound_py_any(py)?;
        Ok(py_int)
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_negint<'py>(&mut self, py: Python<'py>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 1
        let uint = self.decode_length_finite(py, subtype)?;
        let signed_int = -(uint as i128) - 1;
        let py_int = signed_int.into_bound_py_any(py)?;
        Ok(py_int)
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_bytestring<'py>(
        &mut self,
        py: Python<'py>,
        subtype: u8,
    ) -> PyResult<Bound<'py, PyBytes>> {
        // Major tag 2
        let (decoded, length) = match self.decode_length(py, subtype)? {
            None => {
                // Indefinite length
                let mut bytes = PyBytes::new(py, b"");
                let mut total_length: usize = 0;
                let sys_maxsize = *SYS_MAXSIZE.get(py).unwrap();
                loop {
                    let (major_type, subtype) = self.read_major_and_subtype(py)?;
                    match (major_type, subtype) {
                        (2, _) => {
                            let length = self.decode_length_finite(py, subtype)?;
                            if length > sys_maxsize {
                                return raise_cbor_error(
                                    py,
                                    "CBORDecodeValueError",
                                    format!(
                                        "chunk too long in an indefinite bytestring chunk: {length}"
                                    )
                                    .as_str(),
                                );
                            }
                            total_length += length;
                            let chunk = self.read(py, length)?;
                            bytes = bytes.add(chunk)?.cast_into()?;
                        }
                        (7, 31) => break (bytes, total_length), // break marker
                        _ => {
                            return raise_cbor_error(
                                py,
                                "CBORDecodeValueError",
                                format!(
                                    "non-byte string (major type {major_type}) found in indefinite \
                                    length byte string"
                                )
                                .as_str(),
                            );
                        }
                    }
                }
            }
            Some(length) if length <= 65536 => {
                let bytes = self.read(py, length)?;
                (PyBytes::new(py, &bytes), length)
            }
            Some(length) => {
                // Incrementally read the bytestring, in chunks of 65536 bytes
                let mut bytes = PyBytes::new(py, b"");
                let mut remaining_length = length;
                while remaining_length > 0 {
                    let chunk_size = min(remaining_length, 65536) as usize;
                    let chunk = self.read(py, chunk_size)?;
                    remaining_length -= chunk_size;
                    bytes = bytes.add(chunk)?.cast_into()?;
                }
                (bytes, length)
            }
        };
        self.add_string_to_namespace(&decoded, length);
        Ok(decoded)
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_string<'py>(
        &mut self,
        py: Python<'py>,
        subtype: u8,
    ) -> PyResult<Bound<'py, PyString>> {
        // Major tag 3
        let (decoded, length) = match self.decode_length(py, subtype)? {
            None => {
                // Indefinite length
                let mut string = PyString::new(py, "");
                let mut total_length: usize = 0;
                loop {
                    let (major_type, subtype) = self.read_major_and_subtype(py)?;
                    let sys_maxsize = *SYS_MAXSIZE.get(py).unwrap();
                    match (major_type, subtype) {
                        (3, _) => {
                            let length = self.decode_length_finite(py, subtype)?;
                            if length > sys_maxsize {
                                return raise_cbor_error(
                                    py,
                                    "CBORDecodeValueError",
                                    format!(
                                        "chunk too long in an indefinite text string chunk: {length}"
                                    ).as_str(),
                                );
                            }
                            total_length += length;
                            let bytes = self.read(py, length)?;
                            let decoded: Bound<PyString> = bytes
                                .into_bound_py_any(py)?
                                .call_method1(
                                    intern!(py, "decode"),
                                    (intern!(py, "utf-8"), &self.str_errors),
                                )
                                .map_err(|e| {
                                    create_cbor_error(
                                        py,
                                        "CBORDecodeValueError",
                                        "error decoding text string",
                                        Some(e),
                                    )
                                })?
                                .cast_into()
                                .map_err(|e| PyErr::from(e))?;
                            string = string.add(decoded)?.cast_into()?;
                        }
                        (7, 31) => break (string, total_length), // break marker
                        _ => {
                            return raise_cbor_error(
                                py,
                                "CBORDecodeValueError",
                                format!(
                                    "non-text string (major type {major_type}) found in indefinite \
                                    length text string"
                                )
                                .as_str(),
                            );
                        }
                    }
                }
            }
            Some(length) if length <= 65536 => {
                let bytes = self.read(py, length)?;
                let py_bytes = bytes.into_bound_py_any(py)?;
                let decode_result = py_bytes.call_method1(
                    intern!(py, "decode"),
                    (intern!(py, "utf-8"), self.str_errors.bind(py)),
                );
                if let Ok(decoded_bytes) = decode_result {
                    (decoded_bytes.cast_into().map_err(PyErr::from)?, length)
                } else {
                    return raise_cbor_error_from(
                        py,
                        "CBORDecodeValueError",
                        "error decoding text string",
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
                    .call1((self.str_errors.bind(py),))?;
                let mut string = PyString::new(py, "");
                while length > 0 {
                    let chunk_size = min(length, 65536) as usize;
                    let chunk = self.read(py, chunk_size)?;
                    length -= chunk_size;
                    let is_final_chunk = length == 0;
                    let decode_result =
                        decoder.call_method1(intern!(py, "decode"), (chunk, is_final_chunk));
                    let decoded_chunk: Bound<'_, PyString> = match decode_result {
                        Ok(decoded_chunk) => decoded_chunk.cast_into()?,
                        Err(e) => {
                            return raise_cbor_error_from(
                                py,
                                "CBORDecodeValueError",
                                "error decoding text string",
                                e,
                            );
                        }
                    };
                    string = string.add(decoded_chunk)?.cast_into()?;
                }
                (string, length)
            }
        };
        self.add_string_to_namespace(&decoded, length);
        Ok(decoded)
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_array<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 4
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let length = this.decode_length(py, subtype)?;
        match (length, this.immutable) {
            (None, true) => {
                // Tuple of indefinite length
                let mut items = Vec::<Bound<'_, PyAny>>::new();
                drop(this);
                loop {
                    let obj = Self::decode(slf)?;
                    if obj.is_exact_instance_of::<BreakMarkerType>() {
                        let tuple = PyTuple::new(py, items)?;
                        slf.borrow_mut().set_shareable(&tuple);
                        break Ok(tuple.into_any());
                    }
                    items.push(obj);
                }
            }
            (None, false) => {
                // Indefinite length list (shareable)
                let list = PyList::empty(py);
                this.set_shareable(&list);
                drop(this);
                loop {
                    let obj = Self::decode(slf)?;
                    if obj.is_exact_instance_of::<BreakMarkerType>() {
                        break Ok(list.into_any());
                    } else {
                        list.append(obj)?;
                    }
                }
            }
            (Some(length), true) => {
                // Fixed-length tuple
                drop(this);
                let mut items = Vec::<Bound<'_, PyAny>>::with_capacity(length);
                for _ in 0..length {
                    items.push(Self::decode(slf)?);
                }
                let tuple = PyTuple::new(py, items)?;
                slf.borrow_mut().set_shareable(&tuple);
                Ok(tuple.into_any())
            }
            (Some(length), false) => {
                // Fixed-length list (shareable)
                let list = PyList::empty(py);
                this.set_shareable(&list);
                drop(this);
                for _ in 0..length {
                    list.append(Self::decode(slf)?)?;
                }
                Ok(list.into_any())
            }
        }
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_map<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 5
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let dict = PyDict::new(py);
        this.set_shareable(&dict);
        match this.decode_length(py, subtype)? {
            None => {
                // Indefinite length
                drop(this);
                loop {
                    let key = Self::with_immutable(slf, || Self::decode(slf))?;
                    if key.is_exact_instance_of::<BreakMarkerType>() {
                        break;
                    }
                    let value = Self::decode(slf)?;
                    dict.set_item(key, value)?;
                }
            }
            Some(length) => {
                drop(this);
                for _ in 0..length {
                    let key = Self::with_immutable(slf, || Self::decode(slf))?;
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
            let args = PyTuple::new(py, [dict])?;
            FrozenDict::new(&args)?.into_bound_py_any(py)
        } else {
            Ok(dict.into_any())
        }
    }

    fn decode_semantic<'py>(slf: &Bound<'py, Self>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let tagnum = this.decode_length_finite(py, subtype)?;

        if let Some(semantic_decoders) = &this.semantic_decoders {
            match semantic_decoders.bind(py).get_item(&tagnum) {
                Ok(decoder) => {
                    drop(this);
                    return decoder.call1((slf,))
                },
                Err(e) if e.is_instance_of::<PyLookupError>(py) => {}
                Err(e) => return Err(e)
            }
        }

        // No semantic decoder lookup map – fall back to the hard coded switchboard
        drop(this);
        match tagnum {
            0 => Self::decode_datetime_string(slf),
            1 => Self::decode_epoch_datetime(slf),
            2 => Self::decode_positive_bignum(slf),
            3 => Self::decode_negative_bignum(slf),
            4 => Self::decode_fraction(slf),
            5 => Self::decode_bigfloat(slf),
            25 => Self::decode_stringref(slf),
            28 => Self::decode_shareable(slf),
            29 => Self::decode_sharedref(slf),
            30 => Self::decode_rational(slf),
            35 => Self::decode_regexp(slf),
            36 => Self::decode_mime(slf),
            37 => Self::decode_uuid(slf),
            52 => Self::decode_ipv4(slf),
            54 => Self::decode_ipv6(slf),
            100 => Self::decode_epoch_date(slf),
            256 => Self::decode_stringref_namespace(slf),
            258 => Self::decode_set(slf),
            260 => Self::decode_ipaddress(slf),
            261 => Self::decode_ipnetwork(slf),
            1004 => Self::decode_date_string(slf),
            43000 => Self::decode_complex(slf),
            55799 => Self::decode_self_describe_cbor(slf),
            _ => {
                // For a tag with no designated decoder, check if we have a tag hook, and call
                // that with the tag object, using its return value as the decoded value.
                let tag = CBORTag::new(tagnum.into_bound_py_any(py)?, py.None().into_bound(py))?;
                let bound_tag = Bound::new(py, tag)?;
                this = slf.borrow_mut();
                match this.tag_hook.as_ref() {
                    Some(tag_hook) => {
                        let tag_hook = tag_hook.clone_ref(py);
                        drop(this);
                        bound_tag.borrow_mut().value =
                            Self::with_immutable(slf, || Self::decode(slf))?.unbind();
                        tag_hook.bind(py).call1((slf, bound_tag))
                    }
                    None => {
                        this.set_shareable(&bound_tag);
                        drop(this);
                        bound_tag.borrow_mut().value =
                            Self::with_immutable(slf, || Self::decode(slf))?.unbind();
                        Ok(bound_tag.into_any())
                    }
                }
            }
        }
    }

    #[pyo3(signature = (subtype: "int"))]
    fn decode_special<'py>(&mut self, py: Python<'py>, subtype: u8) -> PyResult<Bound<'py, PyAny>> {
        // Major tag 7
        // let py = slf.py();
        match subtype {
            0..20 => {
                let value = subtype.into_pyobject(py)?;
                CBORSimpleValue::new(value)?.into_bound_py_any(py)
            }
            20 => Ok(false.into_bound_py_any(py)?),
            21 => Ok(true.into_bound_py_any(py)?),
            22 => Ok(py.None().into_bound_py_any(py)?),
            23 => Ok(UNDEFINED.get(py).unwrap().into_bound_py_any(py)?),
            24 => {
                let value = self.read_exact::<1>(py)?[0];
                CBORSimpleValue::new(value.into_pyobject(py)?)?.into_bound_py_any(py)
            }
            25 => {
                let bytes = self.read_exact::<2>(py)?;
                f16::from_be_bytes(bytes).to_f32().into_bound_py_any(py)
            }
            26 => {
                let bytes = self.read_exact::<4>(py)?;
                f32::from_be_bytes(bytes).into_bound_py_any(py)
            }
            27 => {
                let bytes = self.read_exact::<8>(py)?;
                f64::from_be_bytes(bytes).into_bound_py_any(py)
            }
            31 => Ok(BREAK_MARKER.get(py).unwrap().into_bound_py_any(py)?),
            _ => {
                let msg = format!("undefined reserved major type 7 subtype 0x{subtype:x}");
                raise_cbor_error(py, "CBORDecodeValueError", msg.as_str())
            }
        }
    }

    //
    // Decoders for semantic tags (major tag 6)
    //

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

        let datetime = DATETIME_FROMISOFORMAT.get(py)?
            .call1((&datetime_str,))
            .map_err(|e| {
                create_cbor_error(
                    py,
                    "CBORDecodeValueError",
                    format!("invalid datetime string: '{datetime_str}'").as_str(),
                    Some(e),
                )
            })?;
        Ok(datetime)
    }

    fn decode_epoch_datetime<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 1
        let py = slf.py();
        let value = Self::decode(slf)?;
        let utc = UTC.get(py)?;
        DATETIME_FROMTIMESTAMP.get(py)?
            .call1((value, utc))
            .map_err(|e| {
                create_cbor_error(
                    py,
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
        let int = int_type.call_method1(intern!(py, "from_bytes"), (value, intern!(py, "big")))?;
        Ok(int)
    }

    fn decode_negative_bignum<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 3
        let py = slf.py();
        let int_type = py.get_type::<PyInt>();
        let value = Self::decode(slf)?;
        let mut int =
            int_type.call_method1(intern!(py, "from_bytes"), (value, intern!(py, "big")))?;
        int = int.neg()?.add(-1)?;
        Ok(int)
    }

    fn decode_fraction<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 4
        let py = slf.py();
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

        let decimal_class = DECIMAL_TYPE.get(py)?;
        let decimal = wrap_cbor_error(
            py,
            "CBORDecodeValueError",
            "error decoding decimal fraction",
            || {
                let exp = tuple.get_item(0)?;
                let sig_tuple = decimal_class
                    .call1((tuple.get_item(1)?,))?
                    .call_method0(intern!(py, "as_tuple"))?
                    .cast_into::<PyTuple>()?;
                let sign = sig_tuple.get_item(0)?;
                let digits = sig_tuple.get_item(1)?;
                let args_tuple = PyTuple::new(py, [sign, digits, exp])?;
                decimal_class.call1((args_tuple,))
            },
        )?;
        Ok(decimal)
    }

    fn decode_bigfloat<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 5
        let py = slf.py();
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

        let decimal_class = DECIMAL_TYPE.get(py)?;
        let decimal = wrap_cbor_error(
            py,
            "CBORDecodeValueError",
            "error decoding bigfloat",
            || {
                let exp = decimal_class.call1((tuple.get_item(0)?,))?;
                let sig = decimal_class.call1((tuple.get_item(1)?,))?;
                let exp = PyInt::new(py, 2).pow(exp, py.None())?;
                sig.mul(exp)
            },
        )?;
        Ok(decimal)
    }

    fn decode_stringref<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 25
        let py = slf.py();
        let index: usize = Self::decode(slf)?.extract()?;

        let this = slf.borrow();
        let stringref_namespace = this.stringref_namespace.as_ref().ok_or_else(|| {
            create_cbor_error(
                py,
                "CBORDecodeValueError",
                "string reference outside of namespace",
                None,
            )
        })?;

        match stringref_namespace.get(index) {
            Some(value) => Ok(value.clone_ref(py).into_bound(py)),
            None => raise_cbor_error(
                py,
                "CBORDecodeValueError",
                format!("string reference {index} not found").as_str(),
            ),
        }
    }

    fn decode_shareable<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 28
        let mut this = slf.borrow_mut();
        let old_index = this.share_index;
        this.share_index = Some(this.shareables.len());
        this.shareables.push(None);
        drop(this);

        match Self::decode(slf) {
            Ok(decoded) => {
                this = slf.borrow_mut();
                this.set_shareable(&decoded);
                this.share_index = old_index;
                Ok(decoded)
            }
            Err(e) => {
                slf.borrow_mut().share_index = old_index;
                Err(e)
            }
        }
    }

    fn decode_sharedref<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 29
        let py = slf.py();
        let index: usize = Self::decode(slf)?.extract()?;
        match slf.borrow().shareables.get(index) {
            None => raise_cbor_error(
                py,
                "CBORDecodeValueError",
                format!("shared reference {index} not found").as_str(),
            ),
            Some(None) => raise_cbor_error(
                py,
                "CBORDecodeValueError",
                format!("shared value {index} has not been initialized").as_str(),
            ),
            Some(Some(shared)) => Ok(shared.clone_ref(py).into_bound(py)),
        }
    }

    fn decode_rational<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 30
        let py = slf.py();
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

        match FRACTION_TYPE.get(py)?.call1(tuple) {
            Ok(fraction) => Ok(fraction),
            Err(e) => {
                raise_cbor_error_from(py, "CBORDecodeValueError", "error decoding rational", e)
            }
        }
    }

    fn decode_regexp<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 35
        let py = slf.py();
        let value = Self::decode(slf)?;
        match RE_COMPILE.get(py)?.call1((value,)) {
            Ok(regexp) => Ok(regexp),
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
        let py = slf.py();
        let value = Self::decode(slf)?;
        let parser = EMAIL_PARSER.get(py)?.call0()?;
        match parser.call_method1(intern!(py, "parsestr"), (value,)) {
            Ok(message) => Ok(message),
            Err(e) => {
                raise_cbor_error_from(py, "CBORDecodeValueError", "error decoding MIME message", e)
            }
        }
    }

    fn decode_uuid<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 37
        let py = slf.py();
        let value = Self::decode(slf)?;
        let kwargs = PyDict::new(py);
        kwargs.set_item(intern!(py, "bytes"), value)?;
        match UUID_TYPE.get(py)?.call((), Some(&kwargs)) {
            Ok(uuid) => Ok(uuid),
            Err(e) => {
                raise_cbor_error_from(py, "CBORDecodeValueError", "error decoding UUID value", e)
            }
        }
    }

    fn decode_ipv4<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 52
        let py = slf.py();
        let value = Self::with_immutable(slf, || Self::decode(slf))?;
        let addr = if let Ok(bytes) = value.cast::<PyBytes>() {
            // The decoded value was a bytestring, so this is an IPv4 address
            IPV4ADDRESS_TYPE.get(py)?.call1((bytes,))?
        } else if let Ok(tuple) = value.cast_into::<PyTuple>()
            && tuple.len() == 2
        {
            // The decoded value was a 2-item array. Check the types of the elements:
            // (int, bytes) -> network
            // (bytes, int) -> interface
            let first_item = tuple.get_item(0)?;
            let second_item = tuple.get_item(1)?;
            if let Ok(prefix) = first_item.cast::<PyInt>()
                && let Ok(address) = second_item.cast::<PyBytes>()
            {
                let mut address_vec: Vec<u8> = address.extract()?;
                address_vec.resize(4, 0);
                IPV4NETWORK_TYPE.get(py)?.call1(((address_vec, prefix),))?
            } else if let Ok(address) = first_item.cast::<PyBytes>()
                && let Ok(prefix) = second_item.cast::<PyInt>()
            {
                IPV4INTERFACE_TYPE.get(py)?.call1(((address, prefix),))?
            } else {
                return raise_cbor_error(
                    py,
                    "CBORDecodeValueError",
                    "error decoding IPv4: invalid types in input array",
                );
            }
        } else {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding IPv4: input value must be a bytestring or an array of 2 elements",
            );
        };
        Ok(addr)
    }

    fn decode_ipv6<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 54
        let py = slf.py();
        let ipv6addr_class = IPV6ADDRESS_TYPE.get(py)?;
        let value = Self::with_immutable(slf, || Self::decode(slf))?;
        let addr = if let Ok(bytes) = value.cast::<PyBytes>() {
            // The decoded value was a bytestring, so this is an IPv6 address
            ipv6addr_class.call1((bytes,))?
        } else if let Ok(tuple) = value.cast_into::<PyTuple>()
            && (2..=3).contains(&tuple.len())
        {
            // The decoded value was a 2-item (or 3 with zone ID) array.
            // Check the types of the elements:
            // (int, bytes) -> network
            // (bytes, int) -> interface
            let first_item = tuple.get_item(0)?;
            let second_item = tuple.get_item(1)?;
            let zone_id = tuple.get_item(2).ok();
            let (class, addr_bytes, prefix) = if let Ok(prefix) = first_item.cast::<PyInt>()
                && let Ok(address) = second_item.cast::<PyBytes>()
            {
                let mut address_vec: Vec<u8> = address.extract()?;
                address_vec.resize(16, 0);
                Ok((
                    IPV6NETWORK_TYPE.get(py)?,
                    PyBytes::new(py, address_vec.as_slice()),
                    prefix,
                ))
            } else if let Ok(address) = first_item.cast_into::<PyBytes>()
                && let Ok(prefix) = second_item.cast::<PyInt>()
            {
                Ok((IPV6INTERFACE_TYPE.get(py)?, address, prefix))
            } else {
                raise_cbor_error(
                    py,
                    "CBORDecodeValueError",
                    "error decoding IPv6: invalid types in input array",
                )
            }?;
            let addr_obj = ipv6addr_class.call1((addr_bytes,))?;

            // Format the zone ID suffix if a zone ID was included
            // (bytes or integer as the last item of a 3-tuple)
            let zone_id_suffix = if let Some(zone_id) = zone_id {
                if let Ok(zone_id_bytes) = zone_id.cast::<PyBytes>() {
                    let zone_id_str = String::from_utf8(zone_id_bytes.as_bytes().to_vec())?;
                    format!("%{zone_id_str}")
                } else if let Ok(zone_id_int) = zone_id.cast::<PyInt>() {
                    format!("%{zone_id_int}")
                } else {
                    return raise_cbor_error(
                        py,
                        "CBORDecodeValueError",
                        "error decoding IPv6: zone ID must be an integer or a bytestring",
                    );
                }
            } else {
                String::default()
            };

            let formatted_addr = format!("{addr_obj}{zone_id_suffix}/{prefix}");
            class.call1((formatted_addr,))?
        } else {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                "error decoding IPv6: input value must be a bytestring or an array of 2 elements",
            );
        };
        Ok(addr)
    }

    fn decode_epoch_date<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 100
        let value = Self::decode(slf)?.extract::<i32>()? + 719163;
        let date = DATE_FROMORDINAL.get(slf.py())?.call1((value,))?;
        Ok(date)
    }

    fn decode_stringref_namespace<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 256
        let mut this = slf.borrow_mut();
        let old_namespace = take(&mut this.stringref_namespace);
        this.stringref_namespace = Some(Vec::new());
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
        Ok(set)
    }

    fn decode_ipaddress<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 260 (deprecated)
        let py = slf.py();
        let value = Self::decode(slf)?.cast_into::<PyBytes>().map_err(|e| {
            create_cbor_error(
                py,
                "CBORDecodeValueError",
                "invalid IP address",
                Some(PyErr::from(e)),
            )
        })?;
        let addr_obj = match value.len()? {
            4 | 16 => {
                IPADDRESS_FUNC.get(py)?.call1((value,))
            }
            6 => Ok(Bound::new(py, CBORTag::new_internal(260, value.into_any()))?.into_any()), // MAC address
            length => raise_cbor_error(
                py,
                "CBORDecodeValueError",
                format!("invalid IP address length ({length})").as_str(),
            ),
        }?;
        Ok(addr_obj)
    }

    fn decode_ipnetwork<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 261 (deprecated)
        let py = slf.py();
        let value: Bound<PyDict> = Self::decode(slf)?.cast_into::<PyDict>()?;
        if value.len() != 1 {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                format!("invalid input map length for IP network: {}", value.len()).as_str(),
            );
        }
        let first_item = value.items().get_item(0)?;
        let mask_length = first_item.get_item(1)?;
        if !mask_length.is_exact_instance_of::<PyInt>() {
            return raise_cbor_error(
                py,
                "CBORDecodeValueError",
                format!("invalid mask length for IP network: {mask_length}").as_str(),
            );
        }

        let addr_obj = match IPNETWORK_FUNC.get(py)?.call1((&first_item,)) {
            Ok(ip_network) => Ok(ip_network),
            Err(e) => {
                // A ValueError may indicate that the bytestring has host bits set, so try parsing
                // it as an IP interface instead
                if e.is_instance_of::<PyValueError>(py) {
                    IPINTERFACE_FUNC.get(py)?.call1((first_item,))
                } else {
                    Err(e)
                }
            }
        }?;
        Ok(addr_obj)
    }

    fn decode_date_string<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 1004
        let value = Self::decode(slf)?;
        let date = DATE_FROMISOFORMAT.get(slf.py())?.call1((value,))?;
        Ok(date)
    }

    fn decode_complex<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
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
            Ok(PyComplex::from_doubles(py, real, imag).into_any())
        })
    }

    fn decode_self_describe_cbor<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 55799
        Self::decode(slf)
    }
}
