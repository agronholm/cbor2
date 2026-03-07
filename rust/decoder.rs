use crate::_cbor2::{BREAK_MARKER, SYS_MAXSIZE, UNDEFINED};
use crate::decoder::DecoderResult::{
    BeginFrame, CompleteFrame, ContinueFrame, Shareable, SharedReference, StringNamespace,
    StringReference, StringValue, Value,
};
use crate::types::{
    BreakMarkerType, CBORDecodeEOF, CBORDecodeError, CBORDecodeValueError, CBORSimpleValue,
    CBORTag, DECIMAL_TYPE, FRACTION_TYPE, IPV4ADDRESS_TYPE, IPV4INTERFACE_TYPE, IPV4NETWORK_TYPE,
    IPV6ADDRESS_TYPE, IPV6INTERFACE_TYPE, IPV6NETWORK_TYPE, UUID_TYPE,
};
use crate::utils::{PyImportable, create_exc_from, raise_exc_from};
use half::f16;
use pyo3::exceptions::{PyException, PyLookupError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBytes, PyCFunction, PyComplex, PyDict, PyFrozenSet, PyInt, PyList, PyListMethods, PyMapping,
    PySet, PyString, PyTuple,
};
use pyo3::{IntoPyObjectExt, Py, PyAny, intern, pyclass};
use std::mem::{replace, take};

#[cfg(not(Py_3_15))]
use crate::types::FrozenDict;

const VALID_STR_ERRORS: [&str; 5] = [
    "strict",
    "ignore",
    "replace",
    "backslashreplace",
    "surrogateescape",
];
const IMMUTABLE_ATTR: &str = "_cbor2_immutable";
const NAME_ATTR: &str = "_cbor2_name";
const SEEK_CUR: u8 = 1;

static DATE_FROMISOFORMAT: PyImportable = PyImportable::new("datetime", "date.fromisoformat");
static DATE_FROMORDINAL: PyImportable = PyImportable::new("datetime", "date.fromordinal");
static DATETIME_FROMISOFORMAT: PyImportable =
    PyImportable::new("datetime", "datetime.fromisoformat");
static DATETIME_FROMTIMESTAMP: PyImportable =
    PyImportable::new("datetime", "datetime.fromtimestamp");
static EMAIL_PARSER: PyImportable = PyImportable::new("email.parser", "Parser");
static INCREMENTAL_UTF8_DECODER: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
static INT_FROMBYTES: PyImportable = PyImportable::new("builtins", "int.from_bytes");
static IPADDRESS_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_address");
static IPNETWORK_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_network");
static IPINTERFACE_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_interface");
static RE_COMPILE: PyImportable = PyImportable::new("re", "compile");
static UTC: PyImportable = PyImportable::new("datetime", "timezone.utc");
#[cfg(Py_3_15)]
static FROZEN_DICT: PyImportable = PyImportable::new("builtins", "frozendict");

enum DecoderResult<'a> {
    BeginFrame(Box<DecoderCallback<'a>>, bool, Option<Bound<'a, PyAny>>),
    ContinueFrame(bool),
    CompleteFrame(Bound<'a, PyAny>),
    Value(Bound<'a, PyAny>),
    StringValue(Bound<'a, PyAny>, usize),
    StringNamespace,
    StringReference(usize),
    Shareable,
    SharedReference(usize),
}

type DecoderCallback<'py> =
    dyn 'py + FnMut(Bound<'py, PyAny>, bool) -> PyResult<DecoderResult<'py>>;

struct StackFrame<'py> {
    immutable: bool,
    decoder_callback: Option<Box<DecoderCallback<'py>>>,
    shareable_index: Option<usize>,
    contains_string_namespace: bool,
}

/// Decorates a function to be a two-stage decoder.
///
/// :param name: the name displayed in a :exc:`CBORDecodeError` raised by the decoder
///     (e.g. "Error decoding thingamajig") where name='thingamajig`)
/// :param immutable: :data:`True` if the item sent to the decoder should be decoded as immutable
#[pyfunction]
#[pyo3(signature = (func=None, /, *, name=None, immutable=false))]
pub fn shareable_decoder<'py>(
    py: Python<'py>,
    func: Option<Py<PyAny>>,
    name: Option<Py<PyString>>,
    immutable: bool,
) -> PyResult<Bound<'py, PyAny>> {
    match func {
        None => PyCFunction::new_closure(
            py,
            None,
            None,
            move |args: &Bound<'_, PyTuple>,
                  _kwargs: Option<&Bound<'_, PyDict>>|
                  -> PyResult<Py<PyAny>> {
                let py = args.py();
                let func = args.get_item(0)?;
                let name = name.as_ref().map(|x| x.clone_ref(py));
                shareable_decoder(py, Some(func.unbind()), name, immutable).map(Bound::unbind)
            },
        )
        .map(|f| f.into_any()),
        Some(func) => {
            let bound_func = func.bind(py);
            if !bound_func.is_callable() {
                return Err(PyTypeError::new_err(format!("{func} is not callable")));
            }
            bound_func.setattr(intern!(py, NAME_ATTR), name)?;
            bound_func.setattr(intern!(py, IMMUTABLE_ATTR), immutable)?;
            Ok(bound_func.clone().into_any())
        }
    }
}

/// The CBORDecoder class implements a fully featured `CBOR`_ decoder with
/// several extensions for handling shared references, big integers, rational
/// numbers and so on. Typically, the class is not used directly, but the
/// :func:`load` and :func:`loads` functions are called to indirectly construct
/// and use the class.
///
/// When the class is constructed manually, the main entry point is:meth:`decode`.
///
/// :param fp: the file to read from (any file-like object opened for reading in binary mode)
/// :param tag_hook:
///     callable that takes 2 arguments: the decoder instance, and the :class:`.CBORTag`
///     to be decoded. This callback is invoked for any tags for which there is no
///     built-in decoder. The return value is substituted for the :class:`.CBORTag`
///     object in the deserialized output
/// :param object_hook:
///     callable that takes 2 arguments: the decoder instance, and a dictionary. This
///     callback is invoked for each deserialized :class:`dict` object. The return value
///     is substituted for the dict in the deserialized output.
/// :param semantic_decoders:
///     An optional mapping for overriding the decoding for select semantic tags.
///     The value is a mapping of semantic tags (integers) to callables that take
///     the decoder instance as the sole argument.
/// :param str_errors:
///     determines how to handle Unicode decoding errors (see the `Error Handlers`_
///     section in the standard library documentation for details)
/// :param read_size: minimum number of bytes to read at once
///     (ignored if ``fp`` is not seekable)
/// :param max_depth:
///     maximum allowed depth for nested containers
///
/// .. _CBOR: https://cbor.io/
#[pyclass(module = "cbor2")]
pub struct CBORDecoder {
    fp: Option<Py<PyAny>>,
    tag_hook: Option<Py<PyAny>>,
    object_hook: Option<Py<PyAny>>,
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
    fp_is_seekable: bool,
}

impl CBORDecoder {
    pub fn new_internal(
        py: Python<'_>,
        fp: Option<&Bound<'_, PyAny>>,
        buffer: Option<Bound<PyBytes>>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
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
            semantic_decoders: semantic_decoders.map(|d| d.clone().unbind()),
            read_method: None,
            buffer: buffer.map(Bound::unbind),
            read_position: 0,
            available_bytes,
            fp_is_seekable: false,
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
        let bytes_to_read = minimum_amount.max(read_size);
        let num_read_bytes = if let Some(read) = self.read_method.as_ref() {
            let bytes_from_fp: Bound<PyBytes> =
                read.bind(py).call1((&bytes_to_read,))?.cast_into()?;
            let num_read_bytes = bytes_from_fp.len()?;
            if num_read_bytes >= minimum_amount {
                return Ok((bytes_from_fp, num_read_bytes));
            }
            num_read_bytes
        } else {
            0
        };
        Err(CBORDecodeEOF::new_err(format!(
            "premature end of stream (expected to read at least {minimum_amount} \
                 bytes, got {num_read_bytes} instead)"
        )))
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
            None => Err(CBORDecodeValueError::new_err(
                "indefinite length not allowed here",
            )),
        }
    }
    //
    // Decoders for major tags (0-7)
    //

    /// Decode the length of the next item.
    ///
    /// This is a low-level operation that may be needed by custom decoder callbacks.
    ///
    /// :param subtype:
    /// :return: the length of the item, or :data:`None` to indicate an indefinite-length item
    fn decode_length(&mut self, py: Python<'_>, subtype: u8) -> PyResult<Option<usize>> {
        let length = match subtype {
            ..24 => Some(subtype as usize),
            24 => Some(self.read_exact::<1>(py)?[0] as usize),
            25 => Some(u16::from_be_bytes(self.read_exact(py)?) as usize),
            26 => Some(u32::from_be_bytes(self.read_exact(py)?) as usize),
            27 => Some(u64::from_be_bytes(self.read_exact(py)?) as usize),
            31 => None,
            _ => {
                return Err(CBORDecodeValueError::new_err(format!(
                    "unknown unsigned integer subtype 0x{subtype:x}"
                )));
            }
        };
        Ok(length)
    }

    fn decode_uint<'py>(&mut self, py: Python<'py>, subtype: u8) -> PyResult<DecoderResult<'py>> {
        // Major tag 0
        let uint = self.decode_length_finite(py, subtype)?;
        Ok(Value(uint.into_bound_py_any(py)?))
    }

    fn decode_negint<'py>(&mut self, py: Python<'py>, subtype: u8) -> PyResult<DecoderResult<'py>> {
        // Major tag 1
        let uint = self.decode_length_finite(py, subtype)?;
        let signed_int = -(uint as i128) - 1;
        Ok(Value(signed_int.into_bound_py_any(py)?))
    }

    fn decode_bytestring<'py>(
        &mut self,
        py: Python<'py>,
        subtype: u8,
    ) -> PyResult<DecoderResult<'py>> {
        // Major tag 2
        match self.decode_length(py, subtype)? {
            None => {
                // Indefinite length
                let mut bytes = PyBytes::new(py, b"");
                let sys_maxsize = *SYS_MAXSIZE.get(py).unwrap();
                loop {
                    let (major_type, subtype) = self.read_major_and_subtype(py)?;
                    match (major_type, subtype) {
                        (2, _) => {
                            let length = self.decode_length_finite(py, subtype)?;
                            if length > sys_maxsize {
                                return Err(CBORDecodeValueError::new_err(format!(
                                    "chunk too long in an indefinite bytestring chunk: {length}"
                                )));
                            }
                            let chunk = self.read(py, length)?;
                            bytes = bytes.add(chunk)?.cast_into()?;
                        }
                        (7, 31) => break Ok(Value(bytes.into_any())), // break marker
                        _ => {
                            return Err(CBORDecodeValueError::new_err(format!(
                                "non-byte string (major type {major_type}) found in indefinite \
                                    length byte string"
                            )));
                        }
                    }
                }
            }
            Some(length) if length <= 65536 => {
                let bytes = self.read(py, length)?;
                Ok(StringValue(PyBytes::new(py, &bytes).into_any(), length))
            }
            Some(length) => {
                // Incrementally read the bytestring, in chunks of 65536 bytes
                let mut bytes = PyBytes::new(py, b"");
                let mut remaining_length = length;
                while remaining_length > 0 {
                    let chunk_size = remaining_length.min(65536);
                    let chunk = self.read(py, chunk_size)?;
                    remaining_length -= chunk_size;
                    bytes = bytes.add(chunk)?.cast_into()?;
                }
                Ok(StringValue(bytes.into_any(), length))
            }
        }
    }

    fn decode_string<'py>(&mut self, py: Python<'py>, subtype: u8) -> PyResult<DecoderResult<'py>> {
        // Major tag 3
        match self.decode_length(py, subtype)? {
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
                                return Err(CBORDecodeValueError::new_err(format!(
                                    "chunk too long in an indefinite text string chunk: {length}"
                                )));
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
                                    let exc =
                                        CBORDecodeValueError::new_err("error decoding text string");
                                    exc.set_cause(py, Some(e));
                                    exc
                                })?
                                .cast_into()
                                .map_err(|e| PyErr::from(e))?;
                            string = string.add(decoded)?.cast_into()?;
                        }
                        (7, 31) => break Ok(Value(string.into_any())), // break marker
                        _ => {
                            return Err(CBORDecodeValueError::new_err(format!(
                                "non-text string (major type {major_type}) found in indefinite \
                                    length text string"
                            )));
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
                    Ok(StringValue(
                        decoded_bytes.cast_into().map_err(PyErr::from)?,
                        length,
                    ))
                } else {
                    raise_exc_from(
                        py,
                        CBORDecodeValueError::new_err("error decoding text string"),
                        Some(decode_result.unwrap_err()),
                    )
                }
            }
            Some(mut length) => {
                // Incrementally decode the string, in chunks of 65536 bytes
                let decoder_class =
                    INCREMENTAL_UTF8_DECODER.get_or_try_init(py, || -> PyResult<Py<PyAny>> {
                        let decoder = py
                            .import("codecs")?
                            .getattr("lookup")?
                            .call1(("utf-8",))?
                            .getattr("incrementaldecoder")?;
                        Ok(decoder.unbind())
                    })?;
                let decoder = decoder_class.bind(py).call1((self.str_errors.bind(py),))?;
                let mut string = PyString::new(py, "");
                while length > 0 {
                    let chunk_size = length.min(65536);
                    let chunk = self.read(py, chunk_size)?;
                    length -= chunk_size;
                    let is_final_chunk = length == 0;
                    let decode_result =
                        decoder.call_method1(intern!(py, "decode"), (chunk, is_final_chunk));
                    let decoded_chunk: Bound<'_, PyString> = match decode_result {
                        Ok(decoded_chunk) => decoded_chunk.cast_into()?,
                        Err(e) => {
                            return raise_exc_from(
                                py,
                                CBORDecodeValueError::new_err("error decoding text string"),
                                Some(e),
                            );
                        }
                    };
                    string = string.add(decoded_chunk)?.cast_into()?;
                }
                Ok(StringValue(string.into_any(), length))
            }
        }
    }

    fn decode_array<'py>(
        &mut self,
        py: Python<'py>,
        subtype: u8,
        immutable: bool,
    ) -> PyResult<DecoderResult<'py>> {
        // Major tag 4
        let optional_length = self.decode_length(py, subtype)?;
        if immutable {
            let mut items: Vec<Bound<'py, PyAny>> = Vec::new();
            let callback: Box<DecoderCallback<'py>> = if let Some(length) = optional_length {
                if length == 0 {
                    return Ok(Value(PyTuple::empty(py).into_any()));
                }

                Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    items.push(item);
                    if items.len() == length {
                        Ok(CompleteFrame(
                            PyTuple::new(py, take(&mut items))?.into_any(),
                        ))
                    } else {
                        Ok(ContinueFrame(false))
                    }
                })
            } else {
                Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    if item.is_exact_instance_of::<BreakMarkerType>() {
                        Ok(CompleteFrame(
                            PyTuple::new(py, take(&mut items))?.into_any(),
                        ))
                    } else {
                        items.push(item);
                        Ok(ContinueFrame(false))
                    }
                })
            };
            Ok(BeginFrame(callback, false, None))
        } else {
            let mut list = PyList::empty(py);
            let container = list.clone().into_any();
            let callback: Box<DecoderCallback<'py>> = if let Some(length) = optional_length {
                if length == 0 {
                    return Ok(Value(PyList::empty(py).into_any()));
                }

                Box::new(move |item, _immutable: bool| {
                    list.append(item)?;
                    if list.len() == length {
                        Ok(CompleteFrame(
                            replace(&mut list, PyList::empty(py)).into_any(),
                        ))
                    } else {
                        Ok(ContinueFrame(false))
                    }
                })
            } else {
                Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    if item.is_exact_instance_of::<BreakMarkerType>() {
                        Ok(CompleteFrame(
                            replace(&mut list, PyList::empty(py)).into_any(),
                        ))
                    } else {
                        list.append(item)?;
                        Ok(ContinueFrame(false))
                    }
                })
            };
            Ok(BeginFrame(callback, false, Some(container)))
        }
    }

    fn decode_map<'py>(
        &mut self,
        py: Python<'py>,
        subtype: u8,
        immutable: bool,
    ) -> PyResult<DecoderResult<'py>> {
        // Major tag 5

        #[cfg(Py_3_15)]
        fn create_frozen_dict<'py>(
            py: Python<'py>,
            items: Vec<(Bound<'py, PyAny>, Bound<'py, PyAny>)>,
        ) -> PyResult<Bound<'py, PyAny>> {
            FROZEN_DICT
                .get(py)?
                .call1((items,))?
                .cast_into()
                .map_err(|e| PyErr::from(e))
        }
        #[cfg(not(Py_3_15))]
        fn create_frozen_dict<'py>(
            py: Python<'py>,
            items: Vec<(Bound<'py, PyAny>, Bound<'py, PyAny>)>,
        ) -> PyResult<Bound<'py, PyAny>> {
            FrozenDict::from_items(py, items).map(|dict| dict.into_any())
        }

        #[inline]
        fn maybe_call_object_hook<'py>(
            py: Python<'py>,
            dict: Bound<'py, PyAny>,
            object_hook: Option<&Py<PyAny>>,
        ) -> PyResult<Bound<'py, PyAny>> {
            if let Some(object_hook) = object_hook {
                object_hook.bind(py).call1((dict,))
            } else {
                Ok(dict)
            }
        }

        let object_hook = self.object_hook.as_ref().map(|hook| hook.clone_ref(py));
        let length_or_none = self.decode_length(py, subtype)?;

        // Return immediately if this is an empty dict
        if let Some(length) = length_or_none
            && length == 0
        {
            let container: Bound<'py, PyAny> = if immutable {
                create_frozen_dict(py, Vec::new())?
            } else {
                PyDict::new(py).into_any()
            };
            let transformed = maybe_call_object_hook(py, container, object_hook.as_ref())?;
            return Ok(Value(transformed));
        };

        let mut key: Option<Bound<'py, PyAny>> = None;
        if immutable {
            let mut items: Vec<(Bound<'py, PyAny>, Bound<'py, PyAny>)> = Vec::new();
            let callback: Box<DecoderCallback<'py>> = if let Some(length) = length_or_none {
                Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    if let Some(key) = key.take() {
                        items.push((key, item));
                        if items.len() == length {
                            let transformed = maybe_call_object_hook(
                                py,
                                create_frozen_dict(py, take(&mut items))?,
                                object_hook.as_ref(),
                            )?;
                            return Ok(CompleteFrame(transformed));
                        }
                        Ok(ContinueFrame(true))
                    } else {
                        key = Some(item);
                        Ok(ContinueFrame(false))
                    }
                })
            } else {
                Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    if item.is_exact_instance_of::<BreakMarkerType>() {
                        let container = create_frozen_dict(py, take(&mut items))?;
                        let transformed =
                            maybe_call_object_hook(py, container.into_any(), object_hook.as_ref())?;
                        return Ok(CompleteFrame(transformed));
                    } else if let Some(key) = key.take() {
                        items.push((key, item));
                        Ok(ContinueFrame(true))
                    } else {
                        key = Some(item);
                        Ok(ContinueFrame(false))
                    }
                })
            };
            Ok(BeginFrame(callback, true, None))
        } else {
            let mut dict = PyDict::new(py);
            let container = dict.clone().into_any();
            let callback: Box<DecoderCallback<'py>> = if let Some(length) = length_or_none {
                Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    if let Some(key) = key.take() {
                        dict.set_item(key, item)?;
                        if dict.len() == length {
                            let dict = replace(&mut dict, PyDict::new(py));
                            let transformed =
                                maybe_call_object_hook(py, dict.into_any(), object_hook.as_ref())?;
                            return Ok(CompleteFrame(transformed));
                        }
                        Ok(ContinueFrame(true))
                    } else {
                        key = Some(item);
                        Ok(ContinueFrame(false))
                    }
                })
            } else {
                Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    if item.is_exact_instance_of::<BreakMarkerType>() {
                        let dict = replace(&mut dict, PyDict::new(py));
                        let transformed =
                            maybe_call_object_hook(py, dict.into_any(), object_hook.as_ref())?;
                        return Ok(CompleteFrame(transformed));
                    } else if let Some(key) = key.take() {
                        dict.set_item(key, item)?;
                        Ok(ContinueFrame(true))
                    } else {
                        key = Some(item);
                        Ok(ContinueFrame(false))
                    }
                })
            };
            Ok(BeginFrame(callback, true, Some(container)))
        }
    }

    fn decode_semantic<'py>(
        &mut self,
        py: Python<'py>,
        subtype: u8,
        immutable: bool,
    ) -> PyResult<DecoderResult<'py>> {
        let tagnum = self.decode_length_finite(py, subtype)?;
        if let Some(semantic_decoders) = &self.semantic_decoders {
            match semantic_decoders.bind(py).get_item(&tagnum) {
                Ok(decoder) => {
                    let name = decoder.getattr_opt(intern!(py, NAME_ATTR))?;

                    // If these attributes are present, this callable was decorated with
                    // @shareable_decoder
                    return if let Some(name) = name {
                        let require_immutable: bool = decoder
                            .getattr_opt(intern!(py, IMMUTABLE_ATTR))?
                            .map(|x| x.is_truthy())
                            .transpose()?
                            .unwrap_or(false);
                        let retval = decoder.call1((immutable,))?;
                        let tuple: Bound<'_, PyTuple> = retval.cast_into()?;
                        if tuple.len() != 2 {
                            return Err(CBORDecodeError::new_err(format!(
                                "{decoder} returned a tuple of {} items, expected 2",
                                tuple.len()
                            )));
                        }
                        let container: Bound<'_, PyAny> = tuple.get_item(0)?.cast_into()?;
                        let callback: Bound<'_, PyAny> = tuple.get_item(1)?.cast_into()?;
                        Ok(BeginFrame(
                            Box::new(
                                move |item, _immutable: bool| -> PyResult<DecoderResult<'py>> {
                                    callback.call1((item,)).map(CompleteFrame)
                                },
                            ),
                            require_immutable,
                            if container.is_none() {
                                None
                            } else {
                                Some(container)
                            },
                        ))
                    } else {
                        let callback =
                            move |item, new_immutable: bool| -> PyResult<DecoderResult<'py>> {
                                decoder.call1((item, new_immutable)).map(CompleteFrame)
                            };
                        Ok(BeginFrame(Box::new(callback), immutable, None))
                    };
                }
                Err(e) if e.is_instance_of::<PyLookupError>(py) => {}
                Err(e) => return Err(e),
            }
        };

        // No semantic decoder lookup map – fall back to the hard coded switchboard
        let callback: Box<DecoderCallback<'py>> = match tagnum {
            0 => Box::new(Self::decode_datetime_string),
            1 => Box::new(Self::decode_epoch_datetime),
            2 => Box::new(Self::decode_positive_bignum),
            3 => Box::new(Self::decode_negative_bignum),
            4 => Box::new(Self::decode_fraction),
            5 => Box::new(Self::decode_bigfloat),
            25 => Box::new(Self::decode_stringref),
            28 => return Ok(Shareable),
            29 => Box::new(Self::decode_sharedref),
            30 => Box::new(Self::decode_rational),
            35 => Box::new(Self::decode_regexp),
            36 => Box::new(Self::decode_mime),
            37 => Box::new(Self::decode_uuid),
            52 => Box::new(Self::decode_ipv4),
            54 => Box::new(Self::decode_ipv6),
            100 => Box::new(Self::decode_epoch_date),
            256 => return Ok(StringNamespace),
            258 => return self.decode_set(py, immutable),
            260 => Box::new(Self::decode_ipaddress),
            261 => Box::new(Self::decode_ipnetwork),
            1004 => Box::new(Self::decode_date_string),
            43000 => Box::new(Self::decode_complex),
            55799 => Box::new(Self::decode_self_describe_cbor),
            _ => {
                // For a tag with no designated decoder, check if we have a tag hook, and call
                // that with the tag object, using its return value as the decoded value.
                let tag = CBORTag::new(tagnum.into_bound_py_any(py)?, py.None().into_bound(py))?;
                let bound_tag = Bound::new(py, tag)?.into_any();
                let container = bound_tag.clone();
                let mut tag_hook = self
                    .tag_hook
                    .as_ref()
                    .map(|hook| hook.clone_ref(py).into_bound(py));
                let callback = Box::new(move |item: Bound<'py, PyAny>, _immutable: bool| {
                    let tag: &Bound<'py, CBORTag> = bound_tag.cast()?;
                    tag.borrow_mut().value = item.unbind();
                    if let Some(tag_hook) = tag_hook.take() {
                        tag_hook.call1((&bound_tag, immutable)).map(CompleteFrame)
                    } else {
                        Ok(CompleteFrame(bound_tag.clone()))
                    }
                });
                return Ok(BeginFrame(callback, true, Some(container)));
            }
        };
        Ok(BeginFrame(callback, true, None))
    }

    fn decode_special<'py>(
        &mut self,
        py: Python<'py>,
        subtype: u8,
    ) -> PyResult<DecoderResult<'py>> {
        // Major tag 7
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
            _ => Err(CBORDecodeValueError::new_err(format!(
                "undefined reserved major type 7 subtype 0x{subtype:x}"
            ))),
        }
        .map(Value)
    }

    //
    // Decoders for semantic tags (major tag 6)
    //

    fn decode_datetime_string<'py>(
        value: Bound<'py, PyAny>,
        _immutable: bool,
    ) -> PyResult<DecoderResult<'py>> {
        // Semantic tag 0
        let py = value.py();
        let value_type = value.get_type();
        let mut datetime_str: Bound<'py, PyString> = value.cast_into().map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err(format!(
                    "expected string for tag, got {} instead",
                    value_type.to_string()
                )),
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

        DATETIME_FROMISOFORMAT
            .get(py)?
            .call1((&datetime_str,))
            .map_err(|e| {
                create_exc_from(
                    py,
                    CBORDecodeValueError::new_err(format!(
                        "invalid datetime string: '{datetime_str}'"
                    )),
                    Some(e),
                )
            })
            .map(CompleteFrame)
    }

    fn decode_epoch_datetime(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 1
        let py = value.py();
        let utc = UTC.get(py)?;
        DATETIME_FROMTIMESTAMP
            .get(py)?
            .call1((value, utc))
            .map(CompleteFrame)
    }

    fn decode_positive_bignum(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 2
        let py = value.py();
        INT_FROMBYTES
            .get(py)?
            .call1((value, intern!(py, "big")))
            .map(CompleteFrame)
    }

    fn decode_negative_bignum(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 3
        let py = value.py();
        let int = INT_FROMBYTES.get(py)?.call1((value, intern!(py, "big")))?;
        int.neg()?.add(-1).map(CompleteFrame)
    }

    fn decode_fraction(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 4
        let py = value.py();
        let tuple = value.cast::<PyTuple>().map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err(
                    "error decoding decimal fraction: input value must be an array",
                ),
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return Err(CBORDecodeValueError::new_err(
                "error decoding decimal fraction: array must have exactly two elements",
            ));
        }

        let decimal_class = DECIMAL_TYPE.get(py)?;
        {
            let exp = tuple.get_item(0)?;
            let sig_tuple = decimal_class
                .call1((tuple.get_item(1)?,))?
                .call_method0(intern!(py, "as_tuple"))?
                .cast_into::<PyTuple>()?;
            let sign = sig_tuple.get_item(0)?;
            let digits = sig_tuple.get_item(1)?;
            let args_tuple = PyTuple::new(py, [sign, digits, exp])?;
            decimal_class.call1((args_tuple,)).map(CompleteFrame)
        }
        .map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err("error decoding decimal fraction"),
                Some(e),
            )
        })
    }

    fn decode_bigfloat(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 5
        let py = value.py();
        let tuple = value.cast::<PyTuple>().map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err(
                    "error decoding bigfloat: input value must be an array",
                ),
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return Err(CBORDecodeValueError::new_err(
                "error decoding bigfloat: array must have exactly two elements",
            ));
        }

        let decimal_class = DECIMAL_TYPE.get(py)?;
        {
            let exp = decimal_class.call1((tuple.get_item(0)?,))?;
            let sig = decimal_class.call1((tuple.get_item(1)?,))?;
            let exp = PyInt::new(py, 2).pow(exp, py.None())?;
            sig.mul(exp).map(CompleteFrame)
        }
        .map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err("error decoding bigfloat"),
                Some(e),
            )
        })
    }

    fn decode_stringref(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 25
        let index: usize = value.extract()?;
        Ok(StringReference(index))
    }

    fn decode_sharedref(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 29
        let index: usize = value.extract()?;
        Ok(SharedReference(index))
    }

    fn decode_rational(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 30
        let py = value.py();
        let tuple = value.cast_into::<PyTuple>().map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err(
                    "error decoding rational: input value must be an array",
                ),
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return Err(CBORDecodeValueError::new_err(
                "error decoding rational: array must have exactly two elements",
            ));
        }

        match FRACTION_TYPE.get(py)?.call1(tuple) {
            Ok(fraction) => Ok(CompleteFrame(fraction)),
            Err(e) => raise_exc_from(
                py,
                CBORDecodeValueError::new_err("error decoding rational"),
                Some(e),
            ),
        }
    }

    fn decode_regexp(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 35
        let py = value.py();
        match RE_COMPILE.get(py)?.call1((value,)) {
            Ok(regexp) => Ok(CompleteFrame(regexp)),
            Err(e) => raise_exc_from(
                py,
                CBORDecodeValueError::new_err("error decoding regular expression"),
                Some(e),
            ),
        }
    }

    fn decode_mime(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 36
        let py = value.py();
        let parser = EMAIL_PARSER.get(py)?.call0()?;
        match parser.call_method1(intern!(py, "parsestr"), (value,)) {
            Ok(message) => Ok(CompleteFrame(message)),
            Err(e) => raise_exc_from(
                py,
                CBORDecodeValueError::new_err("error decoding MIME message"),
                Some(e),
            ),
        }
    }

    fn decode_uuid(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 37
        let py = value.py();
        let kwargs = PyDict::new(py);
        kwargs.set_item(intern!(py, "bytes"), value)?;
        match UUID_TYPE.get(py)?.call((), Some(&kwargs)) {
            Ok(uuid) => Ok(CompleteFrame(uuid)),
            Err(e) => raise_exc_from(
                py,
                CBORDecodeValueError::new_err("error decoding UUID value"),
                Some(e),
            ),
        }
    }

    fn decode_ipv4(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 52
        let py = value.py();
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
                return Err(CBORDecodeValueError::new_err(
                    "error decoding IPv4: invalid types in input array",
                ));
            }
        } else {
            return Err(CBORDecodeValueError::new_err(
                "error decoding IPv4: input value must be a bytestring or an array of 2 elements",
            ));
        };
        Ok(CompleteFrame(addr))
    }

    fn decode_ipv6(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 54
        let py = value.py();
        let ipv6addr_class = IPV6ADDRESS_TYPE.get(py)?;
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
                Err(CBORDecodeValueError::new_err(
                    "error decoding IPv6: invalid types in input array",
                ))
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
                    return Err(CBORDecodeValueError::new_err(
                        "error decoding IPv6: zone ID must be an integer or a bytestring",
                    ));
                }
            } else {
                String::default()
            };

            let formatted_addr = format!("{addr_obj}{zone_id_suffix}/{prefix}");
            class.call1((formatted_addr,))?
        } else {
            return Err(CBORDecodeValueError::new_err(
                "error decoding IPv6: input value must be a bytestring or an array of 2 elements",
            ));
        };
        Ok(CompleteFrame(addr))
    }

    fn decode_epoch_date(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 100
        let py = value.py();
        let value = value.extract::<i32>()? + 719163;
        let date = DATE_FROMORDINAL.get(py)?.call1((value,))?;
        Ok(CompleteFrame(date))
    }

    fn decode_ipaddress(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 260 (deprecated)
        let py = value.py();
        let value = value.cast_into::<PyBytes>().map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err("invalid IP address"),
                Some(PyErr::from(e)),
            )
        })?;
        let addr_obj = match value.len()? {
            4 | 16 => IPADDRESS_FUNC.get(py)?.call1((value,)),
            6 => Ok(Bound::new(py, CBORTag::new_internal(260, value.into_any()))?.into_any()), // MAC address
            length => Err(CBORDecodeValueError::new_err(format!(
                "invalid IP address length ({length})"
            ))),
        }?;
        Ok(CompleteFrame(addr_obj))
    }

    fn decode_ipnetwork<'py>(
        value: Bound<'py, PyAny>,
        _immutable: bool,
    ) -> PyResult<DecoderResult<'py>> {
        // Semantic tag 261 (deprecated)
        let py = value.py();
        let value: Bound<'py, PyMapping> = value.cast_into()?;
        let length = value.len()?;
        if length != 1 {
            return Err(CBORDecodeValueError::new_err(format!(
                "invalid input map length for IP network: {}",
                length
            )));
        }
        let first_item = value.items()?.get_item(0)?;
        let mask_length = first_item.get_item(1)?;
        if !mask_length.is_exact_instance_of::<PyInt>() {
            return Err(CBORDecodeValueError::new_err(format!(
                "invalid mask length for IP network: {mask_length}"
            )));
        }

        let addr_obj = match IPNETWORK_FUNC.get(py)?.call1((&first_item,)) {
            Ok(ip_network) => Ok(ip_network),
            Err(e) => {
                // A CompleteFrameError may indicate that the bytestring has host bits set, so try parsing
                // it as an IP interface instead
                if e.is_instance_of::<PyValueError>(py) {
                    IPINTERFACE_FUNC.get(py)?.call1((first_item,))
                } else {
                    Err(e)
                }
            }
        }?;
        Ok(CompleteFrame(addr_obj))
    }

    fn decode_date_string(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 1004
        let py = value.py();
        let date = DATE_FROMISOFORMAT.get(py)?.call1((value,))?;
        Ok(CompleteFrame(date))
    }

    fn decode_complex(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 43000
        let py = value.py();
        let tuple = value.cast_into::<PyTuple>().map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeValueError::new_err(
                    "error decoding complex: input value must be an array",
                ),
                Some(PyErr::from(e)),
            )
        })?;

        if tuple.len() != 2 {
            return Err(CBORDecodeValueError::new_err(
                "error decoding complex: array must have exactly two elements",
            ));
        }

        let real: f64 = tuple.get_item(0)?.extract()?;
        let imag: f64 = tuple.get_item(1)?.extract()?;
        Ok(CompleteFrame(
            PyComplex::from_doubles(py, real, imag).into_any(),
        ))
    }

    fn decode_self_describe_cbor(value: Bound<PyAny>, _immutable: bool) -> PyResult<DecoderResult> {
        // Semantic tag 55799
        Ok(CompleteFrame(value))
    }

    fn decode_set<'py>(
        &mut self,
        py: Python<'py>,
        immutable: bool,
    ) -> PyResult<DecoderResult<'py>> {
        // Semantic tag 258
        let mut set_or_none = if immutable {
            None
        } else {
            Some(PySet::empty(py)?.into_any())
        };
        let container = set_or_none.as_ref().map(|set| set.clone());
        let callback = move |item: Bound<'py, PyAny>, _immutable: bool| {
            let container: Bound<'py, PyAny> = if let Some(set) = set_or_none.take() {
                set.call_method1(intern!(py, "update"), (item,))?;
                set.into_any()
            } else {
                let tuple = item.cast_into::<PyTuple>()?;
                PyFrozenSet::new(py, tuple)?.into_any()
            };
            Ok(CompleteFrame(container))
        };
        Ok(BeginFrame(Box::new(callback), true, container))
    }
}

#[pymethods]
impl CBORDecoder {
    #[new]
    #[pyo3(signature = (
        fp,
        *,
        tag_hook = None,
        object_hook = None,
        semantic_decoders = None,
        str_errors = "strict",
        read_size = 4096,
        max_depth = 1000,
    ))]
    pub fn new(
        py: Python<'_>,
        fp: &Bound<'_, PyAny>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
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
            raise_exc_from(
                fp.py(),
                PyValueError::new_err("fp must be a readable file-like object"),
                result.err(),
            )
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
    /// :param amount: the number of bytes to read
    #[pyo3(signature = (amount, /))]
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

    /// Decode the next value from the stream.
    ///
    /// :param immutable: if :data:`True`, decode the next item as an immutable type
    ///     (e.g. :class:`tuple` instead of a :class:`list`), if possible
    /// :return: the decoded object
    /// :raises CBORDecodeError: if there is any problem decoding the stream
    #[pyo3(signature = (*, immutable = false))]
    pub fn decode<'py>(&mut self, py: Python<'py>, immutable: bool) -> PyResult<Bound<'py, PyAny>> {
        let mut frames: Vec<StackFrame> = Vec::new();

        fn add_frame<'a>(
            frames: &mut Vec<StackFrame<'a>>,
            max_depth: usize,
            frame: StackFrame<'a>,
        ) -> PyResult<()> {
            if frames.len() == max_depth {
                return Err(CBORDecodeError::new_err(format!(
                    "maximum container nesting depth ({max_depth}) exceeded",
                )));
            }

            frames.push(frame);
            Ok(())
        }

        let mut shareables: Vec<Option<Bound<'py, PyAny>>> = Vec::new();
        let mut string_namespaces: Vec<Vec<Bound<'py, PyAny>>> = Vec::new();
        let mut value: Option<Bound<'py, PyAny>> = None;
        let mut current_immutable: bool = immutable;
        loop {
            let result: PyResult<DecoderResult<'py>> = if let Some(previous_value) = value.take() {
                // Call the decoder callback of the last frame
                let frame = frames.last_mut().unwrap();
                if let Some(decoder_callback) = frame.decoder_callback.as_mut() {
                    decoder_callback(previous_value, frame.immutable)
                } else if frame.contains_string_namespace {
                    string_namespaces
                        .pop()
                        .expect("no string namespaces to pop from");
                    Ok(CompleteFrame(previous_value))
                } else if let Some(shareable_index) = frame.shareable_index {
                    shareables[shareable_index].get_or_insert_with(|| previous_value.clone());
                    Ok(CompleteFrame(previous_value))
                } else {
                    panic!("no decoder callback, shareable index or string namespace");
                }
            } else {
                let (major_type, subtype) = self.read_major_and_subtype(py)?;
                match major_type {
                    0 => self.decode_uint(py, subtype),
                    1 => self.decode_negint(py, subtype),
                    2 => self.decode_bytestring(py, subtype),
                    3 => self.decode_string(py, subtype),
                    4 => self.decode_array(py, subtype, current_immutable),
                    5 => self.decode_map(py, subtype, current_immutable),
                    6 => self.decode_semantic(py, subtype, current_immutable),
                    7 => self.decode_special(py, subtype),
                    _ => Err(CBORDecodeError::new_err(format!(
                        "invalid major type: {major_type}"
                    ))),
                }
            };

            match result {
                Ok(BeginFrame(callback, requested_immutable, container)) => {
                    if let Some(frame) = frames.last_mut()
                        && let Some(container) = container
                        && let Some(shareable_index) = frame.shareable_index
                    {
                        frames.pop();
                        shareables[shareable_index] = Some(container.clone());
                    }
                    current_immutable = current_immutable || requested_immutable;
                    add_frame(
                        &mut frames,
                        self.max_depth,
                        StackFrame {
                            immutable: current_immutable,
                            decoder_callback: Some(callback),
                            shareable_index: None,
                            contains_string_namespace: false,
                        },
                    )?;
                }
                Ok(ContinueFrame(require_immutable)) => {
                    // If require_immutable is true, the next value must be immutable
                    // Otherwise, restore the immutable flag to the previous value
                    current_immutable = if frames.len() >= 2 {
                        frames.get(frames.len() - 2).unwrap().immutable
                    } else {
                        immutable
                    } || require_immutable;
                }
                Ok(CompleteFrame(new_value)) => {
                    frames
                        .pop()
                        .expect("received frame completion but there are no frames on the stack");
                    current_immutable = frames.last().map_or(immutable, |frame| frame.immutable);
                    value = Some(new_value);
                }
                Ok(Value(new_value)) => {
                    value = Some(new_value);
                }
                Ok(StringNamespace) => {
                    add_frame(
                        &mut frames,
                        self.max_depth,
                        StackFrame {
                            immutable: current_immutable,
                            decoder_callback: None,
                            shareable_index: None,
                            contains_string_namespace: true,
                        },
                    )?;
                    string_namespaces.push(Vec::new());
                }
                Ok(StringValue(string, length)) => {
                    // Conditionally add the string to the innermost string namespace
                    if let Some(namespace) = string_namespaces.last_mut() {
                        if match namespace.len() {
                            0..24 => length >= 3,
                            24..256 => length >= 4,
                            256..65536 => length >= 5,
                            65536..4294967296 => length >= 6,
                            _ => length >= 11,
                        } {
                            namespace.push(string.clone());
                        }
                    }
                    value = Some(string);
                }
                Ok(StringReference(index)) => {
                    frames
                        .pop()
                        .expect("  received string reference but there are no frames on the stack");
                    if let Some(namespace) = string_namespaces.last() {
                        if let Some(string) = namespace.get(index) {
                            value = Some(string.clone());
                        } else {
                            return Err(CBORDecodeValueError::new_err(format!(
                                "string reference {index} not found"
                            )));
                        }
                    } else {
                        return Err(CBORDecodeValueError::new_err(
                            "string reference outside of namespace",
                        ));
                    }
                    current_immutable = frames
                        .last()
                        .map_or(current_immutable, |frame| frame.immutable);
                }
                Ok(Shareable) => {
                    add_frame(
                        &mut frames,
                        self.max_depth,
                        StackFrame {
                            immutable: current_immutable,
                            decoder_callback: None,
                            shareable_index: Some(shareables.len()),
                            contains_string_namespace: false,
                        },
                    )?;
                    shareables.push(None);
                }
                Ok(SharedReference(index)) => {
                    frames
                        .pop()
                        .expect("received shared reference but there are no frames on the stack");
                    value = match shareables.get(index) {
                        Some(Some(value)) => Some(value.clone()),
                        Some(None) => {
                            return Err(CBORDecodeError::new_err(format!(
                                "shared value {index} has not been initialized"
                            )));
                        }
                        None => {
                            return Err(CBORDecodeError::new_err(format!(
                                "shared reference {index} not found"
                            )));
                        }
                    };
                    current_immutable = frames
                        .last()
                        .map_or(current_immutable, |frame| frame.immutable);
                }
                Err(err) => {
                    // If an Exception was raised, wrap it in a CBORDecodeError
                    // If a ValueError was raised, wrap it in a CBORDecodeValueError
                    return if err.is_instance_of::<CBORDecodeError>(py) {
                        Err(err)
                    } else if err.is_instance_of::<PyValueError>(py) {
                        Err(create_exc_from(
                            py,
                            CBORDecodeValueError::new_err(err.to_string()),
                            Some(err),
                        ))
                    } else if err.is_instance_of::<PyException>(py) {
                        Err(create_exc_from(
                            py,
                            CBORDecodeError::new_err(err.to_string()),
                            Some(err),
                        ))
                    } else {
                        Err(err)
                    };
                }
            }

            if frames.is_empty() {
                // If fp was seekable and excess data has been read, empty the buffer and
                // rewind the file
                if self.available_bytes > 0
                    && let Some(fp) = &self.fp
                {
                    let offset = -(self.available_bytes as isize);
                    fp.call_method1(py, intern!(py, "seek"), (offset, SEEK_CUR))?;
                    self.buffer = None;
                    self.available_bytes = 0;
                    self.read_position = 0;
                }
                return Ok(value.expect("stack is empty but final return value is missing"));
            }
        }
    }
}
