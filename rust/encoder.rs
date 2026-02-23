use crate::types::{BreakMarkerType, CBORSimpleValue, CBORTag, UndefinedType};
use crate::utils::{PyImportable, raise_cbor_error};
use bigdecimal::BigDecimal;
use half::f16;
use num_bigint::BigInt;
use pyo3::exceptions::{PyLookupError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyCFunction, PyComplex, PyDict, PyFloat, PyFrozenSet, PyInt,
    PyList, PyMapping, PyNone, PySequence, PySet, PyString, PyTuple, PyType,
};
use pyo3::{IntoPyObjectExt, Py, PyAny, intern, pyclass};
use std::collections::HashMap;
use std::mem::swap;

type EncoderFn = fn(&Bound<CBOREncoder>, &Bound<PyAny>) -> PyResult<()>;
type EncoderLookupVec = Vec<(Py<PyType>, EncoderFn)>;

static DATETIME_COMBINE_FUNC: PyImportable = PyImportable::new("datetime", "datetime.combine");
static ID_FUNC: PyImportable = PyImportable::new("builtins", "id");
static TZINFO_TYPE: PyImportable = PyImportable::new("datetime", "tzinfo");
static SORTED_FUNC: PyImportable = PyImportable::new("builtins", "sorted");
static ZERO_TIME: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
static STDLIB_ENCODERS: PyOnceLock<EncoderLookupVec> = PyOnceLock::new();

/// Wrap the given encoder function to gracefully handle cyclic data
/// structures.
///
/// If value sharing is enabled, this marks the given value shared in the
/// datastream on the first call. If the value has already been passed to this
/// method, a reference marker is instead written to the data stream and the
/// wrapped function is not called.
///
/// If value sharing is disabled, only infinite recursion protection is done.
#[pyfunction]
#[pyo3(signature = (wraps, /))]
pub fn shareable_encoder<'py>(
    py: Python<'py>,
    wraps: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyCFunction>> {
    // `wraps` is the original Python function
    let wraps = wraps.clone().unbind();
    PyCFunction::new_closure(
        py,
        None, // no module
        None, // no qualified name override
        move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
            let py = args.py();
            let encoder = args.get_item(0)?.cast_into::<CBOREncoder>()?;
            let value = args.get_item(1)?;
            CBOREncoder::encode_shared(&encoder, &value, || {
                wraps.call1(py, (&encoder, &value)).map(|_| ())
            })
        },
    )
}

/// The CBOREncoder class implements a fully featured CBOR encoder with several extensions for
/// handling shared references, big integers, rational numbers and so on. Typically the class is not
/// used directly, but the dump() and dumps() functions are called to indirectly construct and use
/// the class.
///
/// When the class is constructed manually, the main entry points are :meth:`encode` and
/// :meth:`encode_to_bytes`.
///
/// :param ~typing.IO[bytes] fp:
///     the file to write to (any file-like object opened for writing in binary mode)
/// :param bool datetime_as_timestamp:
///     set to :data:`True` to serialize datetimes as UNIX timestamps (this makes datetimes
///     more concise on the wire, but loses the timezone information)
/// :param ~datetime.tzinfo timezone:
///     the default timezone to use for serializing naive datetimes; if this is not
///     specified naive datetimes will throw a :exc:`ValueError` when encoding is
///     attempted
/// :param bool value_sharing:
///     set to :data:`True` to allow more efficient serializing of repeated values
///     and, more importantly, cyclic data structures, at the cost of extra
///     line overhead
/// :param encoders:
///     An optional mapping for overriding the encoding for select Python types.
///     Each key in this mapping should be a Python type object, and the value a callable
///     that takes two arguments: the encoder object and the object to encode.
/// :type encoders: ~collections.abc.Mapping[type,
///     ~collections.abc.Callable[[CBOREncoder, typing.Any], typing.Any]]
/// :param default:
///     a callable that is called by the encoder with two arguments (the encoder
///     instance and the value being encoded) when no suitable encoder has been found,
///     and should use the methods on the encoder to encode any objects it wants to add
///     to the data stream
/// :type default: ~collections.abc.Callable[[CBOREncoder, typing.Any], None] | None
/// :param bool canonical:
///     when :data:`True`, use "canonical" CBOR representation; this typically involves
///     sorting maps, sets, etc. into a pre-determined order ensuring that
///     serializations are comparable without decoding
/// :param bool date_as_datetime:
///     set to :data:`True` to serialize date objects as datetimes (CBOR tag 0), which was
///     the default behavior in previous releases (cbor2 <= 4.1.2).
/// :param bool string_referencing:
///     set to :data:`True` to allow more efficient serializing of repeated string values
/// :param bool indefinite_containers:
///     encode containers as indefinite (use stop code instead of specifying length)
#[pyclass(module = "cbor2")]
pub struct CBOREncoder {
    fp: Option<Py<PyAny>>,

    #[pyo3(get)]
    datetime_as_timestamp: bool,

    timezone: Option<Py<PyAny>>,

    #[pyo3(get)]
    value_sharing: bool,

    default: Option<Py<PyAny>>,

    #[pyo3(get)]
    canonical: bool,

    #[pyo3(get)]
    date_as_datetime: bool,

    #[pyo3(get)]
    string_referencing: bool,

    #[pyo3(get)]
    string_namespacing: bool,

    #[pyo3(get)]
    indefinite_containers: bool,

    encoders: Option<Py<PyMapping>>,
    write_method: Option<Py<PyAny>>,
    pub buffer: Vec<u8>,
    shared_containers: HashMap<usize, (Py<PyAny>, Option<Py<PyInt>>)>,
    string_references: HashMap<String, usize>,
    bytes_references: HashMap<Vec<u8>, usize>,
    encode_depth: usize,
}

const MAX_BUFFER_SIZE: usize = 4096;

impl CBOREncoder {
    pub fn new_internal(
        fp: Option<&Bound<'_, PyAny>>,
        datetime_as_timestamp: bool,
        timezone: Option<&Bound<'_, PyAny>>,
        value_sharing: bool,
        encoders: Option<&Bound<'_, PyMapping>>,
        default: Option<&Bound<'_, PyAny>>,
        canonical: bool,
        date_as_datetime: bool,
        string_referencing: bool,
        indefinite_containers: bool,
    ) -> PyResult<Self> {
        let mut instance = Self {
            fp: None,
            datetime_as_timestamp,
            timezone: None,
            value_sharing,
            default: None,
            canonical,
            date_as_datetime,
            string_referencing,
            string_namespacing: string_referencing,
            indefinite_containers,
            encoders: encoders.map(|e| e.clone().unbind()),
            write_method: None,
            buffer: Vec::new(),
            shared_containers: HashMap::new(),
            string_references: HashMap::new(),
            bytes_references: HashMap::new(),
            encode_depth: 0,
        };
        if let Some(fp) = fp {
            instance.set_fp(fp)?;
        }
        instance.set_timezone(timezone)?;
        instance.set_default(default)?;
        Ok(instance)
    }

    fn encode_shared(
        slf: &Bound<'_, Self>,
        obj: &Bound<'_, PyAny>,
        f: impl FnOnce() -> PyResult<()>,
    ) -> PyResult<()> {
        let py = slf.py();
        let value_sharing = slf.borrow().value_sharing;
        let value_id = ID_FUNC.get(py)?.call1((obj,))?.extract::<usize>()?;

        let mut this = slf.borrow_mut();
        let option = this.shared_containers.get(&value_id);
        match option {
            None => {
                if value_sharing {
                    // Mark the container as shareable
                    let next_index = PyInt::new(py, this.shared_containers.len()).unbind();
                    this.shared_containers.insert(
                        value_id,
                        (obj.clone().unbind(), Some(next_index.clone_ref(py))),
                    );
                    this.encode_length(py, 6, Some(28))?;
                    drop(this);
                    f().map(|_| ())
                } else {
                    this.shared_containers
                        .insert(value_id, (obj.clone().unbind(), None));
                    drop(this);
                    let result = f();
                    slf.borrow_mut().shared_containers.remove(&value_id);
                    result.map(|_| ())
                }
            }
            Some((_, None)) => {
                raise_cbor_error(py, "CBOREncodeValueError", "cyclic data structure detected")
            }
            Some((_, Some(index))) => {
                // Generate a reference to the previous index instead of
                // encoding this again
                let value = index.clone_ref(py);
                this.encode_length(py, 6, Some(29))?;
                drop(this);
                Self::encode_int(slf, value.bind(py))
            }
        }
    }

    /// Call the given function with value sharing disabled in the encoder.
    fn disable_value_sharing<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> T) -> T {
        let mut this = slf.borrow_mut();
        let old_value_sharing = this.value_sharing;
        this.value_sharing = false;
        drop(this);
        let result = f();
        slf.borrow_mut().value_sharing = old_value_sharing;
        result
    }

    /// Call the given function with string namespacing disabled in the encoder.
    fn disable_string_namespacing<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> T) -> T {
        let mut this = slf.borrow_mut();
        let old_string_namespacing = this.string_namespacing;
        this.string_namespacing = false;
        drop(this);
        let result = f();
        slf.borrow_mut().string_namespacing = old_string_namespacing;
        result
    }

    /// Call the given function with string referencing disabled in the encoder.
    fn disable_string_referencing<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> T) -> T {
        let mut this = slf.borrow_mut();
        let old_string_referencing = this.string_referencing;
        this.string_referencing = false;
        drop(this);
        let result = f();
        slf.borrow_mut().string_referencing = old_string_referencing;
        result
    }

    fn encode_container(
        slf: &Bound<'_, Self>,
        obj: &Bound<'_, PyAny>,
        f: impl FnOnce() -> PyResult<()>,
    ) -> PyResult<()> {
        if slf.borrow().string_namespacing {
            // Create a new string reference domain
            slf.borrow_mut().encode_length(slf.py(), 6, Some(256))?;
        }

        Self::disable_string_namespacing(slf, || Self::encode_shared(slf, obj, f))
    }

    fn fp_write_byte(&mut self, py: Python<'_>, data: u8) -> PyResult<()> {
        self.buffer.push(data);
        self.maybe_flush(py)
    }

    fn maybe_flush(&mut self, py: Python<'_>) -> PyResult<()> {
        if !self.fp.is_none() && self.buffer.len() >= MAX_BUFFER_SIZE {
            self.flush(py)
        } else {
            Ok(())
        }
    }

    fn maybe_stringref(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let (index, is_string) = if let Ok(py_string) = value.cast::<PyString>() {
            let string: String = py_string.extract()?;
            (this.string_references.get(&string).copied(), true)
        } else {
            let bytes: Vec<u8> = value.cast::<PyBytes>()?.extract()?;
            (this.bytes_references.get(&bytes).copied(), false)
        };
        match index {
            Some(index) => {
                drop(this);
                Self::encode_semantic(slf, 25, PyInt::new(py, index).as_any())?;
                Ok(true)
            }
            None => {
                let length = value.len()?;
                let next_index = this.string_references.len() + this.bytes_references.len();
                let is_referenced = match next_index {
                    ..24 => length >= 3,
                    24..256 => length >= 4,
                    256..65536 => length >= 5,
                    65536..4294967296 => length >= 7,
                    _ => length >= 11,
                };

                if is_referenced {
                    if is_string {
                        this.string_references.insert(value.extract()?, next_index);
                    } else {
                        this.bytes_references.insert(value.extract()?, next_index);
                    }
                }

                Ok(false)
            }
        }
    }
}

#[pymethods]
impl CBOREncoder {
    #[new]
    #[pyo3(signature = (
        fp,
        *,
        datetime_as_timestamp = false,
        timezone = None,
        value_sharing = false,
        encoders = None,
        default = None,
        canonical = false,
        date_as_datetime = false,
        string_referencing = false,
        indefinite_containers = false
    ))]
    pub fn new(
        fp: &Bound<'_, PyAny>,
        datetime_as_timestamp: bool,
        timezone: Option<&Bound<'_, PyAny>>,
        value_sharing: bool,
        encoders: Option<&Bound<'_, PyMapping>>,
        default: Option<&Bound<'_, PyAny>>,
        canonical: bool,
        date_as_datetime: bool,
        string_referencing: bool,
        indefinite_containers: bool,
    ) -> PyResult<Self> {
        CBOREncoder::new_internal(
            Some(fp),
            datetime_as_timestamp,
            timezone,
            value_sharing,
            encoders,
            default,
            canonical,
            date_as_datetime,
            string_referencing,
            indefinite_containers,
        )
    }

    #[getter]
    fn fp(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.fp.as_ref().map(|fp| fp.clone_ref(py))
    }

    #[setter]
    fn set_fp(&mut self, fp: &Bound<'_, PyAny>) -> PyResult<()> {
        let result = fp.call_method0("writable");
        if let Ok(writable) = &result
            && writable.is_truthy()?
        {
            // Before replacing the file pointer, flush any pending writes and clear state
            if let Some(existing_fp) = &self.fp
                && !fp.is(existing_fp)
            {
                self.flush(fp.py())?;
                self.shared_containers.clear();
                self.string_references.clear();
                self.bytes_references.clear();
            }

            self.write_method = Some(fp.getattr("write")?.unbind());
            self.fp = Some(fp.clone().unbind());
            Ok(())
        } else {
            let exc = PyValueError::new_err("fp must be a writable file-like object");
            exc.set_cause(fp.py(), result.err());
            Err(exc)
        }
    }

    #[getter]
    fn timezone(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.timezone
            .as_ref()
            .map(|timezone| timezone.clone_ref(py))
    }

    #[setter]
    fn set_timezone(&mut self, timezone: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        if let Some(timezone) = timezone {
            let py = timezone.py();
            if !timezone.is_instance(&TZINFO_TYPE.get(py)?)? {
                return Err(PyErr::new::<PyTypeError, _>(
                    "timezone must be a tzinfo object",
                ));
            }

            self.timezone = Some(timezone.clone().unbind());
        } else {
            self.timezone = None;
        }
        Ok(())
    }

    #[getter]
    fn default(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.default.as_ref().map(|default| default.clone_ref(py))
    }

    #[setter]
    fn set_default(&mut self, default: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        if let Some(default) = default {
            if !default.is_callable() {
                return Err(PyErr::new::<PyTypeError, _>("default must be callable"));
            }

            self.default = Some(default.clone().unbind());
        } else {
            self.default = None;
        }
        Ok(())
    }

    fn flush(&mut self, py: Python<'_>) -> PyResult<()> {
        if let Some(fp) = &self.fp {
            fp.call_method1(py, intern!(py, "write"), (&*self.buffer,))?;
            self.buffer.clear();
        }
        Ok(())
    }

    fn fp_write(&mut self, py: Python<'_>, mut data: Vec<u8>) -> PyResult<()> {
        self.buffer.append(&mut data);
        self.maybe_flush(py)
    }

    /// Write bytes to the data stream.
    ///
    /// :param bytes buf: the bytes to write
    /// :returns: the number of bytes written
    /// :rtype: int
    ///
    /// .. note:: This method will first flush any write-ahead buffer, potentially causing
    ///    the number of written bytes to be higher than the length of the bytes passed
    ///    as the argument.
    #[pyo3(signature = (buf, /))]
    fn write<'py>(&mut self, py: Python<'py>, buf: Vec<u8>) -> PyResult<Bound<'py, PyAny>> {
        if self.write_method.is_none() {
            return Err(PyRuntimeError::new_err("fp not set"));
        }
        self.flush(py)?;
        let write = self.write_method.as_ref().unwrap();
        write.bind(py).call1((&buf,))
    }

    fn encode_value(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Look up the Python type object of the object to be encoded
        let py = slf.py();
        let this = slf.borrow();

        if let Some(encoders) = &this.encoders {
            match encoders.bind(py).get_item(&obj.get_type()) {
                Ok(encoder) => {
                    drop(this);
                    return encoder.call1((slf, obj)).map(|_| ());
                }
                Err(e) if e.is_instance_of::<PyLookupError>(py) => {}
                Err(e) => return Err(e),
            }
        }

        // Look up the type in the encoders dict, and if no encoder callback was found, check for
        // special types. If all else fails, fall back to the default encoder callback, if one was
        // provided. Otherwise, raise CBOREncoderError.
        drop(this);
        if let Ok(obj) = obj.cast::<PyBytes>() {
            Self::encode_bytes(slf, obj)
        } else if let Ok(obj) = obj.cast::<PyString>() {
            Self::encode_string(slf, obj)
        } else if let Ok(obj) = obj.cast::<PyBool>() {
            Self::encode_bool(slf, obj)
        } else if let Ok(obj) = obj.cast::<PyInt>() {
            Self::encode_int(slf, obj)
        } else if let Ok(obj) = obj.cast::<PyFloat>() {
            Self::encode_float(slf, obj)
        } else if let Ok(obj) = obj.cast::<PyComplex>() {
            Self::encode_complex(slf, obj)
        } else if let Ok(obj) = obj.cast::<PyByteArray>() {
            Self::encode_bytearray(slf, obj)
        } else if obj.is_none() {
            Self::encode_none(slf)
        } else if obj.is_exact_instance_of::<UndefinedType>() {
            Self::encode_undefined(slf)
        } else if obj.is_exact_instance_of::<BreakMarkerType>() {
            Self::encode_break(slf)
        } else if let Ok(map) = obj.cast::<PyMapping>() {
            Self::encode_map(slf, map)
        } else if let Ok(sequence) = obj.cast::<PySequence>() {
            Self::encode_array(slf, sequence)
        } else if let Ok(sequence) = obj.cast::<PySet>() {
            Self::encode_set(slf, sequence)
        } else if let Ok(sequence) = obj.cast::<PyFrozenSet>() {
            Self::encode_frozenset(slf, sequence)
        } else if let Ok(simple_value) = obj.cast::<CBORSimpleValue>() {
            Self::encode_simple_value(slf, simple_value)
        } else if let Ok(tag) = obj.cast::<CBORTag>() {
            let tag = tag.borrow();
            Self::encode_semantic(slf, tag.tag, tag.value.bind(py))
        } else {
            let obj_type = obj.get_type();
            let stdlib_encoders =
                STDLIB_ENCODERS.get_or_try_init(py, || -> PyResult<EncoderLookupVec> {
                    let mut encoders: EncoderLookupVec = Vec::new();
                    encoders.push((
                        py.import("datetime")?
                            .getattr("datetime")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_datetime,
                    ));
                    encoders.push((
                        py.import("datetime")?
                            .getattr("date")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_date,
                    ));
                    encoders.push((
                        py.import("decimal")?
                            .getattr("Decimal")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_decimal,
                    ));
                    encoders.push((
                        py.import("fractions")?
                            .getattr("Fraction")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_rational,
                    ));
                    encoders.push((
                        py.import("uuid")?.getattr("UUID")?.cast_into()?.unbind(),
                        CBOREncoder::encode_uuid,
                    ));
                    encoders.push((
                        py.import("re")?.getattr("Pattern")?.cast_into()?.unbind(),
                        CBOREncoder::encode_regexp,
                    ));
                    encoders.push((
                        py.import("ipaddress")?
                            .getattr("IPv4Address")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_ipv4_address,
                    ));
                    encoders.push((
                        py.import("ipaddress")?
                            .getattr("IPv4Network")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_ipv4_network,
                    ));
                    encoders.push((
                        py.import("ipaddress")?
                            .getattr("IPv4Interface")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_ipv4_interface,
                    ));
                    encoders.push((
                        py.import("ipaddress")?
                            .getattr("IPv6Address")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_ipv6_address,
                    ));
                    encoders.push((
                        py.import("ipaddress")?
                            .getattr("IPv6Network")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_ipv6_network,
                    ));
                    encoders.push((
                        py.import("ipaddress")?
                            .getattr("IPv6Interface")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_ipv6_interface,
                    ));
                    encoders.push((
                        py.import("email.mime.text")?
                            .getattr("MIMEText")?
                            .cast_into()?
                            .unbind(),
                        CBOREncoder::encode_mime,
                    ));
                    Ok(encoders)
                })?;
            for (pytype, callback) in stdlib_encoders {
                if obj_type.is(pytype) {
                    return callback(slf, obj);
                }
            }

            let default = slf.borrow().default.as_ref().map(|d| d.clone_ref(py));
            if let Some(default) = default {
                default.call1(py, (slf, obj)).map(|_| ())
            } else {
                raise_cbor_error(
                    py,
                    "CBOREncodeError",
                    format!("cannot encode type {obj_type}").as_str(),
                )
            }
        }
    }

    /// Encode the given object using CBOR.
    ///
    /// :param obj: the object to encode
    #[pyo3(signature = (obj, /))]
    pub fn encode(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        slf.borrow_mut().encode_depth += 1;

        Self::encode_value(slf, obj)?;

        let mut this = slf.borrow_mut();
        this.encode_depth -= 1;
        if this.encode_depth == 0 {
            this.flush(slf.py())?;
            this.shared_containers.clear();
            this.string_references.clear();
            this.bytes_references.clear();
        }
        Ok(())
    }

    /// Encode the given object to a byte buffer and return its value as bytes.
    ///
    /// This method was intended to be used from the ``default`` hook when an
    /// object needs to be encoded separately from the rest but while still
    /// taking advantage of the shared value registry.
    ///
    /// :param obj: the object to encode
    /// :rtype: bytes
    #[pyo3(signature = (obj, /))]
    pub fn encode_to_bytes<'py>(
        slf: &Bound<'py, Self>,
        obj: &Bound<'py, PyAny>,
    ) -> PyResult<Vec<u8>> {
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let mut fp: Option<Py<PyAny>> = None;
        let mut buffer: Vec<u8> = Vec::new();
        swap(&mut this.fp, &mut fp);
        swap(&mut this.buffer, &mut buffer);
        drop(this);

        let result = Self::encode(slf, obj);

        this = slf.borrow_mut();
        this.flush(py)?;
        swap(&mut this.fp, &mut fp);
        swap(&mut this.buffer, &mut buffer);
        result.map(|_| buffer)
    }

    /// Takes a key and calculates the length of its optimal byte
    /// representation, along with the representation itself.
    /// This is used as the sorting key in CBOR's canonical representations.
    fn encode_sortable_key<'py>(
        slf: &Bound<'py, Self>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<(usize, Bound<'py, PyAny>)> {
        Self::disable_string_referencing(slf, || {
            let encoded = Self::encode_to_bytes(slf, &key)?;
            let py_bytes = PyBytes::new(slf.py(), encoded.as_slice());
            Ok((encoded.len(), py_bytes.into_any()))
        })
    }

    /// Takes a (key, value) tuple and calculates the length of its optimal byte
    /// representation, along with the representation itself.
    /// This is used as the sorting key in CBOR's canonical representations.
    ///
    /// :param item: a (key, value) tuple
    /// :type item: tuple[Any, Any]
    fn encode_sortable_item<'py>(
        slf: &Bound<'py, Self>,
        item: &Bound<'py, PyTuple>,
    ) -> PyResult<(usize, Bound<'py, PyAny>)> {
        let key = item.get_item(0)?;
        Self::encode_sortable_key(slf, &key)
    }

    fn encode_length(
        &mut self,
        py: Python<'_>,
        major_tag: u8,
        length: Option<u64>,
    ) -> PyResult<()> {
        let major_tag = major_tag << 5;
        match length {
            Some(len) => match len {
                ..24 => self.fp_write_byte(py, major_tag | len as u8),
                24..256 => {
                    self.fp_write_byte(py, major_tag | 24)?;
                    self.fp_write(py, (len as u8).to_be_bytes().to_vec())
                }
                256..65536 => {
                    self.fp_write_byte(py, major_tag | 25)?;
                    self.fp_write(py, (len as u16).to_be_bytes().to_vec())
                }
                65536..4294967296 => {
                    self.fp_write_byte(py, major_tag | 26)?;
                    self.fp_write(py, (len as u32).to_be_bytes().to_vec())
                }
                _ => {
                    self.fp_write_byte(py, major_tag | 27)?;
                    self.fp_write(py, len.to_be_bytes().to_vec())
                }
            },
            None => {
                // Indefinite
                self.fp_write_byte(py, major_tag | 31)
            }
        }
    }

    fn encode_string(slf: &Bound<'_, Self>, obj: &Bound<'_, PyString>) -> PyResult<()> {
        let py = slf.py();
        let string_referencing = slf.borrow().string_referencing;

        // If string referencing is enabled, check if this string already has an index,
        // and emit a string reference instead if it does
        if string_referencing {
            if Self::maybe_stringref(slf, obj)? {
                return Ok(());
            }
        }

        let mut this = slf.borrow_mut();
        let encoded = obj.to_str()?.as_bytes();
        this.encode_length(py, 3, Some(encoded.len() as u64))?;
        this.fp_write(py, encoded.to_vec())
    }

    fn encode_bytes(slf: &Bound<'_, Self>, obj: &Bound<'_, PyBytes>) -> PyResult<()> {
        let py = slf.py();
        let string_referencing = slf.borrow().string_referencing;

        // If string referencing is enabled, check if this string already has an index,
        // and emit a string reference instead if it does
        if string_referencing {
            if Self::maybe_stringref(slf, obj)? {
                return Ok(());
            }
        }

        let mut this = slf.borrow_mut();
        let bytes = obj.as_bytes();
        this.encode_length(py, 2, Some(bytes.len() as u64))?;
        this.fp_write(py, bytes.to_vec())
    }

    fn encode_bytearray(slf: &Bound<'_, Self>, obj: &Bound<'_, PyByteArray>) -> PyResult<()> {
        let py = slf.py();
        let mut this = slf.borrow_mut();
        this.encode_length(py, 2, Some(obj.len() as u64))?;
        this.fp_write(py, obj.to_vec())
    }

    fn encode_array(slf: &Bound<'_, Self>, obj: &Bound<'_, PySequence>) -> PyResult<()> {
        Self::encode_container(slf, obj, || {
            let indefinite_containers = slf.borrow().indefinite_containers;
            slf.borrow_mut().encode_length(
                slf.py(),
                4,
                if !indefinite_containers {
                    Some(obj.len()? as u64)
                } else {
                    None
                },
            )?;

            for value in obj.try_iter()? {
                Self::encode_value(slf, &value?)?;
            }

            if indefinite_containers {
                Self::encode_break(slf)?;
            }
            Ok(())
        })
    }

    fn encode_map(slf: &Bound<'_, Self>, obj: &Bound<'_, PyMapping>) -> PyResult<()> {
        Self::encode_container(slf, obj, || {
            let py = slf.py();
            let indefinite_containers = slf.borrow().indefinite_containers;
            slf.borrow_mut().encode_length(
                py,
                5,
                if !indefinite_containers {
                    Some(obj.len()? as u64)
                } else {
                    None
                },
            )?;

            let mut iterator = obj.call_method0("items")?.try_iter()?;
            if slf.borrow().canonical {
                // Reorder keys according to Canonical CBOR specification where they're sorted
                // by the length of the CBOR encoded value first, and only then by the lexical order
                let kwargs = PyDict::new(py);
                kwargs.set_item("key", slf.getattr("encode_sortable_item")?)?;
                iterator = SORTED_FUNC
                    .get(py)?
                    .call((iterator,), Some(&kwargs))?
                    .try_iter()?;
            }
            for item in iterator {
                let (key, value): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item?.extract()?;
                Self::encode_value(slf, &key)?;
                Self::encode_value(slf, &value)?;
            }

            if indefinite_containers {
                Self::encode_break(slf)?
            }
            Ok(())
        })
    }

    fn encode_break(slf: &Bound<'_, Self>) -> PyResult<()> {
        // Break stop code for indefinite containers
        slf.borrow_mut().fp_write_byte(slf.py(), 0xff)
    }

    fn encode_int(slf: &Bound<'_, Self>, obj: &Bound<'_, PyInt>) -> PyResult<()> {
        let py = slf.py();
        if obj.ge(18446744073709551616_i128)? {
            let (_, payload) = obj.extract::<BigInt>()?.to_bytes_be();
            let py_payload = PyBytes::new(py, &payload);
            Self::encode_semantic(slf, 2, py_payload.as_any())
        } else if obj.lt(-18446744073709551616_i128)? {
            let mut value = obj.extract::<BigInt>()?;
            value = -value - 1;
            let (_, payload) = value.to_bytes_be();
            let py_payload = PyBytes::new(py, &payload);
            Self::encode_semantic(slf, 3, py_payload.as_any())
        } else if obj.ge(0)? {
            let value: u64 = obj.extract()?;
            slf.borrow_mut().encode_length(py, 0, Some(value))
        } else {
            let value = obj.add(1)?.abs()?.extract::<u64>()?;
            slf.borrow_mut().encode_length(py, 1, Some(value))
        }
    }

    fn encode_bool(slf: &Bound<'_, Self>, obj: &Bound<'_, PyBool>) -> PyResult<()> {
        slf.borrow_mut()
            .fp_write_byte(slf.py(), if obj.is_true() { b'\xf5' } else { b'\xf4' })
    }

    fn encode_none(slf: &Bound<'_, Self>) -> PyResult<()> {
        slf.borrow_mut().fp_write_byte(slf.py(), b'\xf6')
    }

    fn encode_undefined(slf: &Bound<'_, Self>) -> PyResult<()> {
        slf.borrow_mut().fp_write_byte(slf.py(), b'\xf7')
    }

    /// Encode a value with a semantic tag.
    ///
    /// :param int tag: a numeric tag value
    /// :param value: the object to be encoded
    fn encode_semantic(slf: &Bound<'_, Self>, tag: u64, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let old_string_referencing = slf.borrow().string_referencing;
        if tag == 256 {
            let mut this = slf.borrow_mut();
            this.string_referencing = true;

            // TODO: move the string/bytestring references here temporarily
        }
        let mut result = slf.borrow_mut().encode_length(slf.py(), 6, Some(tag));
        if result.is_ok() {
            result = Self::encode(slf, &value);
        }
        slf.borrow_mut().string_referencing = old_string_referencing;
        // TODO: restore the string/bytestring references to the instance
        result
    }

    fn encode_set(slf: &Bound<'_, Self>, obj: &Bound<'_, PySet>) -> PyResult<()> {
        // Semantic tag 258
        if slf.borrow().canonical {
            let py = slf.py();
            let kwargs = PyDict::new(py);
            kwargs.set_item("key", slf.getattr("encode_sortable_key")?)?;
            let list = SORTED_FUNC.get(py)?.call((obj,), Some(&kwargs))?;
            Self::encode_semantic(slf, 258, list.as_any())
        } else {
            let tuple = PyTuple::new(slf.py(), obj)?;
            Self::encode_semantic(slf, 258, tuple.as_any())
        }
    }

    fn encode_frozenset(slf: &Bound<'_, Self>, obj: &Bound<'_, PyFrozenSet>) -> PyResult<()> {
        // Semantic tag 258
        if slf.borrow().canonical {
            let py = slf.py();
            let kwargs = PyDict::new(py);
            kwargs.set_item("key", slf.getattr("encode_sortable_key")?)?;
            let list = SORTED_FUNC.get(py)?.call((obj,), Some(&kwargs))?;
            Self::encode_semantic(slf, 258, list.as_any())
        } else {
            let tuple = PyTuple::new(slf.py(), obj)?;
            Self::encode_semantic(slf, 258, tuple.as_any())
        }
    }

    //
    // Semantic decoders (major tag 6)
    //

    fn encode_datetime(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();

        let inner_encode_datetime = |aware_datetime: &Bound<'_, PyAny>| -> PyResult<()> {
            let datetime_as_timestamp = slf.borrow().datetime_as_timestamp;
            match datetime_as_timestamp {
                false => {
                    let formatted = aware_datetime
                        .call_method0(intern!(py, "isoformat"))?
                        .call_method1(
                            intern!(py, "replace"),
                            (intern!(py, "+00:00"), intern!(py, "Z")),
                        )?;
                    Self::encode_semantic(slf, 0, formatted.as_any())
                }
                true => {
                    let py_timestamp = aware_datetime.call_method0(intern!(py, "timestamp"))?;

                    // If the timestamp can be converted to an integer without loss, encode that
                    // integer instead
                    let timestamp_float: f64 = py_timestamp.extract()?;
                    let timestamp_int: u32 = timestamp_float as u32;
                    let arg: Bound<'_, PyAny>;
                    if timestamp_int as f64 == timestamp_float {
                        arg = PyInt::new(py, timestamp_int).into_any();
                    } else {
                        arg = py_timestamp;
                    }
                    Self::encode_semantic(slf, 1, &arg)
                }
            }
        };

        if obj.getattr("tzinfo")?.is_none() {
            // value is a naive datetime (no time zone)
            let timezone = slf.borrow().timezone.as_ref().map(|tz| tz.clone_ref(py));
            match timezone {
                Some(timezone) => {
                    let kwargs = PyDict::new(py);
                    kwargs.set_item("tzinfo", timezone)?;
                    let value = obj.call_method("replace", (), Some(&kwargs))?;
                    inner_encode_datetime(&value)
                }
                None => raise_cbor_error(
                    py,
                    "CBOREncodeError",
                    "naive datetime encountered and no default timezone has been set",
                ),
            }
        } else {
            inner_encode_datetime(obj)
        }
    }

    fn encode_date(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 100
        let (date_as_datetime, datetime_as_timestamp) = {
            let this = slf.borrow();
            (this.date_as_datetime, this.datetime_as_timestamp)
        };

        if date_as_datetime {
            // Encode a datetime with a zeroed-out time portion
            let py = slf.py();
            let time_zero = ZERO_TIME.get_or_try_init(py, || {
                Ok::<_, PyErr>(py.import("datetime")?.getattr("time")?.call0()?.unbind())
            })?;
            let value = DATETIME_COMBINE_FUNC.get(py)?.call1((obj, time_zero))?;
            Self::encode_datetime(slf, &value)
        } else if datetime_as_timestamp {
            // Encode a date as a number of days since the Unix epoch
            // The baseline has to be adjusted as date.toordinal() returns the number of days from
            // the beginning of the ISO calendar
            let days_since_epoch: i32 = obj.call_method0("toordinal")?.extract()?;
            let adjusted_delta = PyInt::new(slf.py(), days_since_epoch - 719163);
            Self::encode_semantic(slf, 100, &adjusted_delta)
        } else {
            let datestring = obj.call_method0("isoformat")?;
            Self::encode_semantic(slf, 1004, &datestring)
        }
    }

    fn encode_rational(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 30
        let numerator = obj.getattr("numerator")?;
        let denominator = obj.getattr("denominator")?;
        Self::disable_value_sharing(slf, || {
            let tuple = PyTuple::new(slf.py(), &[numerator, denominator])?;
            Self::encode_semantic(slf, 30, &tuple)
        })
    }

    fn encode_regexp(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 35
        let pattern = obj.getattr("pattern")?;
        Self::encode_semantic(slf, 35, &pattern.str()?.as_any())
    }

    fn encode_mime(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 36
        let string = obj.call_method0("as_string")?;
        Self::encode_semantic(slf, 36, &string)
    }

    fn encode_uuid(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 37
        let bytes = obj.getattr("bytes")?;
        Self::encode_semantic(slf, 37, &bytes)
    }

    fn encode_decimal(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        if obj.call_method0("is_nan")?.is_truthy()? {
            slf.borrow_mut().fp_write(slf.py(), vec![0xf9, 0x7e, 0x00])
        } else if obj.call_method0("is_infinite")?.is_truthy()? {
            let signed = obj.call_method0("is_signed")?.is_truthy()?;
            let middle = if signed { 0xfc } else { 0x7c };
            slf.borrow_mut()
                .fp_write(slf.py(), vec![0xf9, middle, 0x00])
        } else {
            let py = slf.py();
            let decimal: BigDecimal = obj.extract()?;
            let (digits, exp) = decimal.as_bigint_and_exponent();
            let py_exp = (-exp).into_bound_py_any(py)?;
            let py_digits = digits.into_bound_py_any(py)?;
            let parts = PyTuple::new(py, &[py_exp, py_digits])?;
            Self::encode_semantic(slf, 4, &parts)
        }
    }

    fn encode_ipv4_address(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 52
        Self::encode_semantic(slf, 52, &obj.getattr("packed")?)
    }

    fn encode_ipv4_network(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 52
        let packed_addr = obj
            .getattr("network_address")?
            .getattr("packed")?
            .call_method1("rstrip", (b"\x00",))?;
        let prefixlen = obj.getattr("prefixlen")?;
        let elements = PyTuple::new(slf.py(), &[prefixlen, packed_addr])?;
        Self::encode_semantic(slf, 52, &elements)
    }

    fn encode_ipv4_interface(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 52
        let packed_addr = obj.getattr("packed")?;
        let prefixlen = obj.getattr("network")?.getattr("prefixlen")?;
        let elements = PyTuple::new(slf.py(), [packed_addr, prefixlen])?;
        Self::encode_semantic(slf, 52, &elements)
    }

    fn encode_ipv6_address(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 54
        let packed_addr = obj.getattr("packed")?;
        let scope_id = obj.getattr("scope_id")?;
        if scope_id.is_none() {
            Self::encode_semantic(slf, 54, &obj.getattr("packed")?)
        } else {
            // Scoped (addr, prefixlen, scope ID)
            let scope_id = scope_id.str()?;
            let none = PyNone::get(slf.py());
            let elements = PyTuple::new(
                slf.py(),
                [&packed_addr, &none, &scope_id.encode_utf8()?.into_any()],
            )?;
            Self::encode_semantic(slf, 54, &elements)
        }
    }

    fn encode_ipv6_network(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 54
        let py = slf.py();
        let packed_addr = obj
            .getattr("network_address")?
            .getattr("packed")?
            .call_method1("rstrip", (b"\x00",))?;
        let prefixlen = obj.getattr("prefixlen")?;
        let elements = PyTuple::new(py, [prefixlen, packed_addr])?;
        Self::encode_semantic(slf, 54, &elements)
    }

    fn encode_ipv6_interface(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 54
        let packed_addr = obj.getattr("packed")?;
        let prefixlen = obj.getattr("network")?.getattr("prefixlen")?;
        let scope_id = obj.getattr("scope_id")?;
        let elements = PyList::new(slf.py(), [packed_addr, prefixlen])?;
        if !scope_id.is_none() {
            elements.append(scope_id.cast_into::<PyString>()?.encode_utf8()?)?;
        }
        Self::encode_semantic(slf, 54, &elements)
    }

    //
    // Special encoders (major tag 7)
    //

    fn encode_simple_value(
        slf: &Bound<'_, Self>,
        obj: &Bound<'_, CBORSimpleValue>,
    ) -> PyResult<()> {
        let py = slf.py();
        let value = obj.get().0;
        if value < 24 {
            slf.borrow_mut().fp_write_byte(py, 0xe0 | value)
        } else {
            slf.borrow_mut().fp_write(py, vec![0xf8, value])
        }
    }

    fn encode_float(slf: &Bound<'_, Self>, obj: &Bound<'_, PyFloat>) -> PyResult<()> {
        let py = slf.py();
        let value = obj.extract::<f64>()?;
        if value.is_nan() {
            slf.borrow_mut().fp_write(py, vec![0xf9, 0x7e, 0x00])
        } else if value.is_infinite() {
            let middle = if value.is_sign_positive() { 0x7c } else { 0xfc };
            slf.borrow_mut().fp_write(py, vec![0xf9, middle, 0x00])
        } else {
            if slf.borrow().canonical {
                // Find the shortest form that did not lose precision with the cast
                let value_32 = value as f32;
                if value_32 as f64 == value {
                    let value_16 = f16::from_f32(value_32);
                    return if value_16.to_f32() == value_32 {
                        slf.borrow_mut().fp_write_byte(py, 0xf9)?;
                        slf.borrow_mut()
                            .fp_write(py, value_16.to_be_bytes().to_vec())
                    } else {
                        slf.borrow_mut().fp_write_byte(py, 0xfa)?;
                        slf.borrow_mut()
                            .fp_write(py, value_32.to_be_bytes().to_vec())
                    };
                }
            }
            slf.borrow_mut().fp_write_byte(py, 0xfb)?;
            slf.borrow_mut().fp_write(py, value.to_be_bytes().to_vec())
        }
    }

    fn encode_complex(slf: &Bound<'_, Self>, obj: &Bound<'_, PyComplex>) -> PyResult<()> {
        let tuple = PyTuple::new(slf.py(), [obj.real(), obj.imag()])?;
        Self::encode_semantic(slf, 43000, tuple.as_any())
    }
}
