use crate::types::{BreakMarkerType, CBORSimpleValue, CBORTag, UndefinedType};
use crate::utils::raise_cbor_error;
use bigdecimal::BigDecimal;
use half::f16;
use num_bigint::BigInt;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyByteArray, PyBytes, PyComplex, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PyMapping,
    PyNone, PySequence, PySet, PyString, PyTuple,
};
use pyo3::{IntoPyObjectExt, Py, PyAny, intern, pyclass};
use std::collections::HashMap;

#[pyclass(module = "cbor2")]
pub struct CBOREncoder {
    fp: Py<PyAny>,

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

    #[pyo3(get)]
    encoders: Py<PyDict>,

    buffer: Vec<u8>,
    shared_containers: HashMap<usize, (Py<PyAny>, Option<Py<PyInt>>)>,
    string_references: HashMap<String, u64>,
    bytes_references: HashMap<String, u64>,
}

const MAX_BUFFER_SIZE: usize = 4096;

pub fn with(
    cm: &Bound<'_, PyAny>,
    f: impl FnOnce(Bound<'_, PyAny>) -> PyResult<()>,
) -> PyResult<()> {
    let py = cm.py();
    let enter_fn = cm.getattr("__enter__")?;
    let exit_fn = cm.getattr("__exit__")?;
    let managed = enter_fn.call0()?;
    match f(managed) {
        Ok(_) => {
            let none = py.None();
            exit_fn.call1((&none, &none, &none)).map(|_| ())
        }
        Err(exc) => {
            let exit_result =
                exit_fn.call1((exc.get_type(py), exc.value(py), exc.traceback(py)))?;
            match exit_result.is_truthy()? {
                true => Ok(()),
                false => Err(exc),
            }
        }
    }
}

impl CBOREncoder {
    fn encode_shared_internal(
        slf: &Bound<'_, Self>,
        obj: &Bound<'_, PyAny>,
        f: impl FnOnce() -> PyResult<()>,
    ) -> PyResult<()> {
        let py = slf.py();
        let value_sharing = slf.borrow().value_sharing;
        let id = py.import("builtins")?.getattr("id")?;
        let value_id = id.call1((obj,))?.extract::<usize>()?;

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
                let exc = py
                    .import("cbor2._types")?
                    .getattr("CBOREncodeValueError")?
                    .call1(("cyclic data structure detected but value sharing is disabled",))?;
                Err(PyErr::from_value(exc))
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
    pub fn disable_value_sharing<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> T) -> T {
        let mut this = slf.borrow_mut();
        let old_value_sharing = this.value_sharing;
        this.value_sharing = false;
        drop(this);
        let result = f();
        slf.borrow_mut().value_sharing = old_value_sharing;
        result
    }

    /// Call the given function with string namespacing disabled in the encoder.
    pub fn disable_string_namespacing<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> T) -> T {
        let mut this = slf.borrow_mut();
        let old_string_namespacing = this.string_namespacing;
        this.string_namespacing = false;
        drop(this);
        let result = f();
        slf.borrow_mut().string_namespacing = old_string_namespacing;
        result
    }

    /// Call the given function with string referencing disabled in the encoder.
    pub fn disable_string_referencing<T>(slf: &Bound<'_, Self>, f: impl FnOnce() -> T) -> T {
        let mut this = slf.borrow_mut();
        let old_string_referencing = this.string_referencing;
        this.string_referencing = false;
        drop(this);
        let result = f();
        slf.borrow_mut().string_referencing = old_string_referencing;
        result
    }

    pub fn encode_container_internal(
        slf: &Bound<'_, Self>,
        obj: &Bound<'_, PyAny>,
        f: impl FnOnce() -> PyResult<()>,
    ) -> PyResult<()> {
        if slf.borrow().string_namespacing {
            // Create a new string reference domain
            slf.borrow_mut().encode_length(slf.py(), 6, Some(256))?;
        }

        Self::disable_string_namespacing(slf, || Self::encode_shared_internal(slf, obj, f))
    }
}

#[pymethods]
impl CBOREncoder {
    #[new]
    #[pyo3(signature = (
        fp: "typing.IO[bytes]",
        *,
        datetime_as_timestamp: "bool" = false,
        timezone: "datetime.tzinfo | None" = None,
        value_sharing: "bool" = false,
        default: "collections.abc.Callable[[CBOREncoder, typing.Any], None] | None" = None,
        canonical: "bool" = false,
        date_as_datetime: "bool" = false,
        string_referencing: "bool" = false,
        indefinite_containers: "bool" = false
    ))]
    pub fn new(
        fp: &Bound<'_, PyAny>,
        datetime_as_timestamp: bool,
        timezone: Option<&Bound<'_, PyAny>>,
        value_sharing: bool,
        default: Option<&Bound<'_, PyAny>>,
        canonical: bool,
        date_as_datetime: bool,
        string_referencing: bool,
        indefinite_containers: bool,
    ) -> PyResult<Self> {
        let encoders: Bound<'_, PyDict> =
            fp.py().import("cbor2")?.getattr("encoders")?.cast_into()?;
        let mut instance = Self {
            fp: fp.clone().unbind(),
            datetime_as_timestamp,
            timezone: timezone.map(|tz| tz.clone().unbind()),
            value_sharing,
            default: default.map(|dflt| dflt.clone().unbind()),
            canonical,
            date_as_datetime,
            string_referencing,
            string_namespacing: string_referencing,
            indefinite_containers,
            encoders: encoders.copy()?.unbind(),
            buffer: Vec::new(),
            shared_containers: HashMap::new(),
            string_references: HashMap::new(),
            bytes_references: HashMap::new(),
        };
        instance.set_fp(fp)?;
        instance.set_timezone(timezone)?;
        instance.set_default(default)?;
        Ok(instance)
    }

    #[getter]
    fn fp(&self, py: Python<'_>) -> Py<PyAny> {
        self.fp.clone_ref(py)
    }

    #[setter]
    fn set_fp(&mut self, fp: &Bound<'_, PyAny>) -> PyResult<()> {
        let result = fp.getattr("write");
        if let Ok(write) = result
            && write.is_callable()
        {
            self.fp = fp.clone().unbind();
            Ok(())
        } else {
            Err(PyValueError::new_err(
                "fp must be a file-like object with a write() method",
            ))
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
            let tzinfo = timezone.py().import("datetime")?.getattr("tzinfo")?;
            if !timezone.is_instance(&tzinfo)? {
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
        self.fp.call_method1(py, "write", (&*self.buffer,))?;
        self.buffer.clear();
        Ok(())
    }

    fn maybe_flush(&mut self, py: Python<'_>) -> PyResult<()> {
        if self.buffer.len() >= MAX_BUFFER_SIZE {
            self.flush(py)
        } else {
            Ok(())
        }
    }

    fn fp_write(&mut self, py: Python<'_>, mut data: Vec<u8>) -> PyResult<()> {
        self.buffer.append(&mut data);
        self.maybe_flush(py)
    }

    fn fp_write_byte(&mut self, py: Python<'_>, data: u8) -> PyResult<()> {
        self.buffer.push(data);
        self.maybe_flush(py)
    }

    #[pyo3(signature = (bytes: "bytes", /))]
    pub fn write(&mut self, py: Python<'_>, bytes: Vec<u8>) -> PyResult<()> {
        self.fp.call_method1(py, "write", (&bytes,)).map(|_| ())
    }

    pub fn encode_value(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        // Look up the Python type object of the object to be encoded
        let py = slf.py();
        let obj_type = obj.get_type();
        let result = slf.borrow().encoders.bind(slf.py()).get_item(&obj_type)?;

        // Look up the type in the encoders dict, and if no encoder callback was found, check for
        // special types. If all else fails, fall back to the default encoder callback, if one was
        // provided. Otherwise, raise CBOREncoderError.
        if let Some(encoder_func) = result {
            encoder_func.call1((slf, obj)).map(|_| ())
        } else if obj.is_none() {
            Self::encode_none(slf)
        } else if obj.is_exact_instance_of::<UndefinedType>() {
            Self::encode_undefined(slf)
        } else if obj.is_exact_instance_of::<BreakMarkerType>() {
            Self::encode_break(slf)
        } else if let Ok(simple_value) = obj.cast::<CBORSimpleValue>() {
            Self::encode_simple_value(slf, simple_value)
        } else if let Ok(tag) = obj.cast::<CBORTag>() {
            let tag = tag.get();
            Self::encode_semantic(slf, tag.tag, tag.value.bind(py))
        } else if let Ok(map) = obj.cast::<PyMapping>() {
            Self::encode_map(slf, map)
        } else if let Ok(sequence) = obj.cast::<PySequence>() {
            Self::encode_array(slf, sequence)
        } else {
            let default = slf.borrow().default.as_ref().map(|d| d.clone_ref(py));
            if let Some(default) = default {
                default.call1(py, (slf, obj)).map(|_| ())
            } else {
                let exc = py
                    .import("cbor2._types")?
                    .getattr("CBOREncodeError")?
                    .call1((format!("cannot encode type {obj_type}"),))?;
                Err(PyErr::from_value(exc))
            }
        }
    }

    pub fn encode(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        Self::encode_value(slf, obj)?;
        slf.borrow_mut().flush(slf.py())
    }

    /// Encode the given object to a byte buffer and return its value as bytes.
    ///
    /// This method was intended to be used from the ``default`` hook when an
    /// object needs to be encoded separately from the rest but while still
    /// taking advantage of the shared value registry.
    pub fn encode_to_bytes<'py>(
        slf: &Bound<'py, Self>,
        obj: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let py = slf.py();
        let mut this = slf.borrow_mut();
        let old_fp = this.fp.clone_ref(py);
        let old_buffer = this.buffer.clone();
        let fp = slf.py().import("io")?.getattr("BytesIO")?.call0()?;
        this.fp = fp.unbind();
        drop(this);
        let result = Self::encode(slf, obj);
        this = slf.borrow_mut();
        this.fp = old_fp;
        this.buffer = old_buffer;
        match result {
            Ok(()) => {
                let py_buffer = this.fp.call_method0(py, "getvalue")?;
                Ok(py_buffer.clone_ref(py).into_bound(py).cast_into()?)
            }
            Err(err) => Err(err),
        }
    }

    #[pyo3(signature = (
        encoder: "Callable[[CBOREncoder, typing.Any], typing.Any]",
        value: "typing.Any"
    ))]
    pub fn encode_shared(
        slf: &Bound<'_, Self>,
        encoder: &Bound<'_, PyAny>,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        Self::encode_shared_internal(slf, value, || encoder.call1((slf, value)).map(|_| ()))
    }

    /// Takes a key and calculates the length of its optimal byte
    /// representation, along with the representation itself.
    /// This is used as the sorting key in CBOR's canonical representations.
    pub fn encode_sortable_key<'py>(
        slf: &Bound<'py, Self>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<(usize, Bound<'py, PyAny>)> {
        Self::disable_string_referencing(slf, || {
            let encoded = Self::encode_to_bytes(slf, value)?;
            Ok((encoded.len()?, encoded.cast_into()?))
        })
    }

    #[pyo3(signature = (major_tag: "int", length: "int | None" = None))]
    pub fn encode_length(
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

    pub fn encode_string(slf: &Bound<'_, Self>, obj: &Bound<'_, PyString>) -> PyResult<()> {
        let py = slf.py();
        let encoded = obj.to_str()?.as_bytes();
        slf.borrow_mut()
            .encode_length(py, 3, Some(encoded.len() as u64))?;
        slf.borrow_mut().fp_write(py, encoded.to_vec())
    }

    pub fn encode_bytes(slf: &Bound<'_, Self>, obj: &Bound<'_, PyBytes>) -> PyResult<()> {
        let py = slf.py();
        let bytes = obj.as_bytes();
        slf.borrow_mut()
            .encode_length(py, 2, Some(bytes.len() as u64))?;
        slf.borrow_mut().fp_write(py, bytes.to_vec())
    }

    pub fn encode_bytearray(slf: &Bound<'_, Self>, obj: &Bound<'_, PyByteArray>) -> PyResult<()> {
        let py = slf.py();
        slf.borrow_mut()
            .encode_length(py, 2, Some(obj.len() as u64))?;
        slf.borrow_mut().fp_write(py, obj.to_vec())
    }

    fn encode_array(slf: &Bound<'_, Self>, obj: &Bound<'_, PySequence>) -> PyResult<()> {
        Self::encode_container_internal(slf, obj, || {
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
        Self::encode_container_internal(slf, obj, || {
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
                let sorted_func = py.import("builtins")?.getattr("sorted")?;
                iterator = sorted_func.call1((iterator,))?.try_iter()?;
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

    pub fn encode_break(slf: &Bound<'_, Self>) -> PyResult<()> {
        // Break stop code for indefinite containers
        slf.borrow_mut().fp_write_byte(slf.py(), 0xff)
    }

    pub fn encode_int(slf: &Bound<'_, Self>, integer: &Bound<'_, PyInt>) -> PyResult<()> {
        let py = slf.py();
        if integer.ge(18446744073709551616_i128)? {
            let (_, payload) = integer.extract::<BigInt>()?.to_bytes_be();
            let py_payload = PyBytes::new(py, &payload);
            Self::encode_semantic(slf, 2, py_payload.as_any())
        } else if integer.lt(-18446744073709551616_i128)? {
            let mut value = integer.extract::<BigInt>()?;
            value = -value - 1;
            let (_, payload) = value.to_bytes_be();
            let py_payload = PyBytes::new(py, &payload);
            Self::encode_semantic(slf, 3, py_payload.as_any())
        } else if integer.ge(0)? {
            let value: u64 = integer.extract()?;
            slf.borrow_mut().encode_length(py, 0, Some(value))
        } else {
            let value = integer.add(1)?.abs()?.extract::<u64>()?;
            slf.borrow_mut().encode_length(py, 1, Some(value))
        }
    }

    pub fn encode_bool(slf: &Bound<'_, Self>, value: bool) -> PyResult<()> {
        slf.borrow_mut()
            .fp_write_byte(slf.py(), if value { b'\xf5' } else { b'\xf4' })
    }

    pub fn encode_none(slf: &Bound<'_, Self>) -> PyResult<()> {
        slf.borrow_mut().fp_write_byte(slf.py(), b'\xf6')
    }

    pub fn encode_undefined(slf: &Bound<'_, Self>) -> PyResult<()> {
        slf.borrow_mut().fp_write_byte(slf.py(), b'\xf7')
    }

    pub fn encode_semantic(
        slf: &Bound<'_, Self>,
        tag: u64,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
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

    pub fn encode_set(slf: &Bound<'_, Self>, value: &Bound<'_, PySet>) -> PyResult<()> {
        // Semantic tag 258
        Self::encode_semantic(slf, 258, PyTuple::new(slf.py(), value)?.as_any())
    }

    pub fn encode_frozenset(slf: &Bound<'_, Self>, value: &Bound<'_, PyFrozenSet>) -> PyResult<()> {
        // Semantic tag 258
        Self::encode_semantic(slf, 258, PyTuple::new(slf.py(), value)?.as_any())
    }

    //
    // Semantic decoders (major tag 6)
    //

    pub fn encode_datetime(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();

        let inner_encode_datetime = |aware_datetime: &Bound<'_, PyAny>| -> PyResult<()> {
            let datetime_as_timestamp = slf.borrow().datetime_as_timestamp;
            match datetime_as_timestamp {
                false => {
                    let formatted = aware_datetime
                        .call_method0("isoformat")?
                        .call_method1("replace", (intern!(py, "+00:00"), intern!(py, "Z")))?;
                    Self::encode_semantic(slf, 0, formatted.as_any())
                }
                true => {
                    let py_timestamp = aware_datetime.call_method0("timestamp")?;

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

        if value.getattr("tzinfo")?.is_none() {
            // value is a naive datetime (no time zone)
            let timezone = slf.borrow().timezone.as_ref().map(|tz| tz.clone_ref(py));
            match timezone {
                Some(timezone) => {
                    let kwargs = PyDict::new(py);
                    kwargs.set_item("tzinfo", timezone)?;
                    let value = value.call_method("replace", (), Some(&kwargs))?;
                    inner_encode_datetime(&value)
                }
                None => raise_cbor_error(
                    py,
                    "CBOREncodeError",
                    "naive datetime encountered and no default timezone has been set",
                ),
            }
        } else {
            inner_encode_datetime(value)
        }
    }

    pub fn encode_date(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 100
        let (date_as_datetime, datetime_as_timestamp) = {
            let this = slf.borrow();
            (this.date_as_datetime, this.datetime_as_timestamp)
        };

        if date_as_datetime {
            // Encode a datetime with a zeroed-out time portion
            let py = slf.py();
            let datetime_type = py.import("datetime")?.getattr("datetime")?;
            let time = py.import("datetime")?.getattr("time")?;
            let time_zero = time.call0()?;
            let value = datetime_type.call_method1("combine", (value, time_zero))?;
            Self::encode_datetime(slf, &value)
        } else if datetime_as_timestamp {
            // Encode a date as a number of days since the Unix epoch
            // The baseline has to be adjusted as date.toordinal() returns the number of days from
            // the beginning of the ISO calendar
            let days_since_epoch: i32 = value.call_method0("toordinal")?.extract()?;
            let adjusted_delta = PyInt::new(slf.py(), days_since_epoch - 719163);
            Self::encode_semantic(slf, 100, &adjusted_delta)
        } else {
            let datestring = value.call_method0("isoformat")?;
            Self::encode_semantic(slf, 1004, &datestring)
        }
    }

    pub fn encode_rational(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 30
        let numerator = value.getattr("numerator")?;
        let denominator = value.getattr("denominator")?;
        Self::disable_value_sharing(slf, || {
            let tuple = PyTuple::new(slf.py(), &[numerator, denominator])?;
            Self::encode_semantic(slf, 30, &tuple)
        })
    }

    pub fn encode_regexp(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 35
        let pattern = value.getattr("pattern")?;
        Self::encode_semantic(slf, 35, &pattern.str()?.as_any())
    }

    pub fn encode_mime(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 36
        let string = value.call_method0("as_string")?;
        Self::encode_semantic(slf, 36, &string)
    }

    pub fn encode_uuid(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 37
        let bytes = value.getattr("bytes")?;
        Self::encode_semantic(slf, 37, &bytes)
    }

    pub fn encode_decimal(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if value.call_method0("is_nan")?.is_truthy()? {
            slf.borrow_mut().fp_write(slf.py(), vec![0xf9, 0x7e, 0x00])
        } else if value.call_method0("is_infinite")?.is_truthy()? {
            let signed = value.call_method0("is_signed")?.is_truthy()?;
            let middle = if signed { 0xfc } else { 0x7c };
            slf.borrow_mut()
                .fp_write(slf.py(), vec![0xf9, middle, 0x00])
        } else {
            let py = slf.py();
            let decimal: BigDecimal = value.extract()?;
            let (digits, exp) = decimal.as_bigint_and_exponent();
            let py_exp = (-exp).into_bound_py_any(py)?;
            let py_digits = digits.into_bound_py_any(py)?;
            let parts = PyTuple::new(py, &[py_exp, py_digits])?;
            Self::encode_semantic(slf, 4, &parts)
        }
    }

    pub fn encode_ipv4_address(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 52
        Self::encode_semantic(slf, 52, &value.getattr("packed")?)
    }

    pub fn encode_ipv4_network(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 52
        let packed_addr = value
            .getattr("network_address")?
            .getattr("packed")?
            .call_method1("rstrip", (b"\x00",))?;
        let prefixlen = value.getattr("prefixlen")?;
        let elements = PyTuple::new(slf.py(), &[prefixlen, packed_addr])?;
        Self::encode_semantic(slf, 52, &elements)
    }

    pub fn encode_ipv4_interface(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 52
        let packed_addr = value.getattr("packed")?;
        let prefixlen = value.getattr("network")?.getattr("prefixlen")?;
        let elements = PyTuple::new(slf.py(), [packed_addr, prefixlen])?;
        Self::encode_semantic(slf, 52, &elements)
    }

    pub fn encode_ipv6_address(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 54
        let packed_addr = value.getattr("packed")?;
        let scope_id = value.getattr("scope_id")?;
        if scope_id.is_none() {
            Self::encode_semantic(slf, 54, &value.getattr("packed")?)
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

    pub fn encode_ipv6_network(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 54
        let py = slf.py();
        let packed_addr = value
            .getattr("network_address")?
            .getattr("packed")?
            .call_method1("rstrip", (b"\x00",))?;
        let prefixlen = value.getattr("prefixlen")?;
        let elements = PyTuple::new(py, [prefixlen, packed_addr])?;
        Self::encode_semantic(slf, 54, &elements)
    }

    pub fn encode_ipv6_interface(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        // Semantic tag 54
        let packed_addr = value.getattr("packed")?;
        let prefixlen = value.getattr("network")?.getattr("prefixlen")?;
        let scope_id = value.getattr("scope_id")?;
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

    fn encode_float(slf: &Bound<'_, Self>, value: &Bound<'_, PyFloat>) -> PyResult<()> {
        let py = slf.py();
        let value = value.extract::<f64>()?;
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
                    if value_16.to_f32() == value_32 {
                        slf.borrow_mut().fp_write_byte(py, 0xf9)?;
                        return slf
                            .borrow_mut()
                            .fp_write(py, value_16.to_be_bytes().to_vec());
                    } else {
                        slf.borrow_mut().fp_write_byte(py, 0xfa)?;
                        return slf
                            .borrow_mut()
                            .fp_write(py, value_32.to_be_bytes().to_vec());
                    }
                }
            }
            slf.borrow_mut().fp_write_byte(py, 0xfb)?;
            slf.borrow_mut().fp_write(py, value.to_be_bytes().to_vec())
        }
    }

    fn encode_complex(slf: &Bound<'_, Self>, value: &Bound<'_, PyComplex>) -> PyResult<()> {
        let tuple = PyTuple::new(slf.py(), [value.real(), value.imag()])?;
        Self::encode_semantic(slf, 43000, tuple.as_any())
    }
}
