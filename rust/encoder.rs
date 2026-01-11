use crate::types::{BreakMarkerType, CBORSimpleValue, CBORTag, UndefinedType};
use num_bigint::BigInt;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyComplex, PyFloat, PyInt, PyMapping, PySequence, PySet, PyString, PyTuple};
use pyo3::{pyclass, Py, PyAny};
use std::collections::HashMap;

#[pyclass(subclass, module = "cbor2")]
pub struct CBOREncoder {
    pub fp: Py<PyAny>,

    #[pyo3(get)]
    pub datetime_as_timestamp: bool,

    pub timezone: Option<Py<PyAny>>,

    #[pyo3(get)]
    pub value_sharing: bool,

    #[pyo3(get)]
    pub default: Option<Py<PyAny>>,

    #[pyo3(get)]
    pub canonical: bool,

    #[pyo3(get)]
    pub date_as_datetime: bool,

    #[pyo3(get)]
    pub string_referencing: bool,

    #[pyo3(get)]
    pub indefinite_containers: bool,

    buffer: Vec<u8>,
    shared_containers: HashMap<Py<PyInt>, (Py<PyAny>, Option<usize>)>,
    string_references: HashMap<String, u64>,
    bytes_references: HashMap<String, u64>,
}

const MAX_BUFFER_SIZE: usize = 4096;


// pub fn call_contextmanager<T>(cm: &Bound<'_, PyAny>, f: impl FnOnce(&Bound<'_, PyAny>) -> PyResult<T>) -> PyResult<T> {
//     let managed = cm.call_method0("__enter__")?;
//     let result = f(&managed);
//     let py = cm.py();
//     match result {
//         Ok(_) => {
//             let none = py.None();
//             managed.call_method1("__exit__", (&none, &none, &none))
//         },
//         Err(exc) => {
//             managed.call_method1("__exit__", (exc.get_type(py), exc, exc.traceback(py)))
//         },
//     }?;
//     result
// }

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
        let mut instance = Self {
            fp: fp.clone().unbind(),
            datetime_as_timestamp,
            timezone: timezone.map(|tz| tz.clone().unbind()),
            value_sharing,
            default: default.map(|dflt| dflt.clone().unbind()),
            canonical,
            date_as_datetime,
            string_referencing,
            indefinite_containers,
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
        if fp.is_none() || !fp.hasattr("write")? {
            return Err(PyErr::new::<PyValueError, _>(
                "fp must be a file-like object with a write() method",
            ));
        }

        self.fp = fp.clone().unbind();
        Ok(())
    }

    #[getter]
    fn timezone(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.timezone.as_ref().map(|timezone| timezone.clone_ref(py))
    }

    #[setter]
    fn set_timezone(&mut self, timezone: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        if let Some(timezone) = timezone {
            let tzinfo = timezone.py().import("datetime")?.getattr("tzinfo")?;
            if !timezone.is_instance(&tzinfo)? {
                return Err(PyErr::new::<PyTypeError, _>("timezone must be a tzinfo object"));
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

            self.timezone = Some(default.clone().unbind());
        } else {
            self.timezone = None;
        }
        Ok(())
    }

    /// Disable value sharing in the encoder for the duration of the context
    /// block.
    // pub fn disable_value_sharing(&mut self, f: fn()) {
    //     let old_value_sharing = self.value_sharing;
    //     self.value_sharing = false;
    //     let result = f();
    //     self.value_sharing = old_value_sharing;
    //     result
    // }

    fn flush(&mut self, py: Python<'_>) -> PyResult<()> {
        self.fp.call_method1(py, "write", (&self.buffer,))?;
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

    pub fn encode_value(&mut self, py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(string) = obj.cast::<PyString>() {
            self.encode_string(py, string)
        } else if let Ok(bytes) = obj.cast::<PyBytes>() {
            self.encode_bytes(py, bytes)
        } else if let Ok(bool) = obj.cast::<PyBool>() {
            self.encode_bool(py, bool.is_true())
        } else if let Ok(integer) = obj.cast::<PyInt>() {
            self.encode_int(py, integer)
        } else if let Ok(float) = obj.cast::<PyFloat>() {
            self.encode_float(py, float)
        } else if let Ok(complex) = obj.cast::<PyComplex>() {
            self.encode_complex(py, complex)
        } else if let Ok(map) = obj.cast::<PyMapping>() {
            self.encode_map(py, map)
        } else if let Ok(sequence) = obj.cast::<PySequence>() {
            self.encode_array(py, sequence)
        } else if let Ok(set) = obj.cast::<PySet>() {
            self.encode_set(py, set)
        } else if let Ok(simple_value) = obj.cast::<CBORSimpleValue>() {
            self.encode_simple_value(py, simple_value)
        } else if let Ok(tag) = obj.cast::<CBORTag>() {
            let tag = tag.get();
            self.encode_semantic(py, tag.tag, tag.value.bind(py))
        } else if obj.is_none() {
            self.encode_none(py)
        } else if obj.is_exact_instance_of::<UndefinedType>() {
            self.encode_undefined(py)
        } else if obj.is_exact_instance_of::<BreakMarkerType>() {
            self.encode_break(py)
        // } else if let Some(default) = self.default {
        //     default.call1(py, (self.into_pyobject(py)?, obj,))?;
        //     Ok(())
        } else {
            Err(PyTypeError::new_err(format!(
                "cannot encode type {}",
                obj.get_type().to_string()
            )))
        }
    }

    pub fn encode(&mut self, py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        self.encode_value(py, obj)?;
        self.flush(py)
    }

    // pub fn encode_shared(&mut self, py: Python<'_>, encoder: &Bound<'_, PyAny>, value: &Bound<'_, PyAny>) -> PyResult<()> {
    //     let id = py.import("builtins")?.getattr("id")?;
    //     let value_id = id.call1((value,))?;
    //     let value = value.unbind();
    //     match self.shared_containers.get(&value_id.unbind()) {
    //         Some((_, index)) => {
    //             match index {
    //                 Some(index) => {
    //                     // Generate a reference to the previous index instead of
    //                     // encoding this again
    //                     self.encode_length(py, 6, Some(0x1D))?;
    //                     self.encode_int(py, index)
    //                 }
    //                 None => {
    //                     Err(PyErr::new::<CBOREncodeValueError, _>("cyclic data structure detected but value sharing is disabled"))
    //                 }
    //             }
    //         }
    //         None => {
    //             if self.value_sharing {
    //                 // Mark the container as shareable
    //                 //         self._shared_containers[value_id] = (
    //                 //             value,
    //                 //             len(self._shared_containers),
    //                 //         )
    //                 //         self.encode_length(6, 0x1C)
    //                 encoder.call1((self, value))?;
    //                 self.shared_containers.insert(value_id, (value.unbind(), self.shared_containers.len()));
    //                 Ok(())
    //             } else {
    //                 self.shared_containers.insert(value_id, (value.unbind(), None));
    //                 Ok(())
    //                 //         try:
    //                 //             encoder(self, value)
    //                 //         finally:
    //                 //             del self._shared_containers[value_id]
    //             }
    //         }
    //     }
    // }

    pub fn encode_length(
        &mut self,
        py: Python<'_>,
        mut major_tag: u8,
        length: Option<u64>,
    ) -> PyResult<()> {
        major_tag <<= 5;
        match length {
            Some(len) => match len {
                ..24 => {
                    self.fp_write_byte(py, major_tag | len as u8)?;
                }
                24..256 => {
                    self.fp_write_byte(py, major_tag | 24)?;
                    self.fp_write(py, (len as u8).to_be_bytes().to_vec())?;
                }
                256..65536 => {
                    self.fp_write_byte(py, major_tag | 25)?;
                    self.fp_write(py, (len as u16).to_be_bytes().to_vec())?;
                }
                65536..4294967296 => {
                    self.fp_write_byte(py, major_tag | 26)?;
                    self.fp_write(py, (len as u32).to_be_bytes().to_vec())?;
                }
                _ => {
                    self.fp_write_byte(py, major_tag | 27)?;
                    self.fp_write(py, len.to_be_bytes().to_vec())?;
                }
            },
            None => {
                // Indefinite
                self.buffer.push(major_tag | 31);
            }
        }
        Ok(())
    }

    pub fn encode_string(&mut self, py: Python<'_>, obj: &Bound<'_, PyString>) -> PyResult<()> {
        let encoded = obj.to_str()?.as_bytes();
        self.encode_length(py, 3, Some(encoded.len() as u64))?;
        self.fp_write(py, encoded.to_vec())
    }

    pub fn encode_bytes(&mut self, py: Python<'_>, obj: &Bound<'_, PyBytes>) -> PyResult<()> {
        let bytes = obj.as_bytes();
        self.encode_length(py, 2, Some(bytes.len() as u64))?;
        self.fp_write(py, bytes.to_vec())
    }

    fn encode_array(&mut self, py: Python<'_>, obj: &Bound<'_, PySequence>) -> PyResult<()> {
        self.encode_length(
            py,
            4,
            if !self.indefinite_containers {
                Some(obj.len()? as u64)
            } else {
                None
            },
        )?;
        for value in obj.try_iter()? {
            self.encode_value(py, &value?)?;
        }

        if self.indefinite_containers {
            self.encode_break(py)?;
        }
        Ok(())
    }

    fn encode_map(&mut self, py: Python<'_>, obj: &Bound<'_, PyMapping>) -> PyResult<()> {
        self.encode_length(
            py,
            5,
            if !self.indefinite_containers {
                Some(obj.len()? as u64)
            } else {
                None
            },
        )?;
        for item in obj.items()?.try_iter()? {
            let (key, value): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item?.extract()?;
            self.encode_value(py, &key)?;
            self.encode_value(py, &value)?;
        }

        if self.indefinite_containers {
            self.encode_break(py)?
        }
        Ok(())
    }

    pub fn encode_break(&mut self, py: Python<'_>) -> PyResult<()> {
        // Break stop code for indefinite containers
        self.fp_write_byte(py, 0xff)
    }

    pub fn encode_int(&mut self, py: Python<'_>, integer: &Bound<'_, PyInt>) -> PyResult<()> {
        if let Ok(value) = integer.extract::<i64>() {
            if value >= 0 {
                self.encode_length(py, 0, Some(value as u64))
            } else {
                self.encode_length(py, 1, Some(-(value + 1) as u64))
            }
        } else {
            let mut value: BigInt = integer.extract()?;
            let tag: u64;
            if value >= BigInt::ZERO {
                tag = 0x02;
            } else {
                tag = 0x03;
                value = -value - 1;
            };
            let (_, payload) = value.to_bytes_be();
            let py_payload = PyBytes::new(py, &payload);
            self.encode_semantic(py, tag, py_payload.as_any())
        }
    }

    pub fn encode_bool(&mut self, py: Python<'_>, value: bool) -> PyResult<()> {
        self.fp_write_byte(py, if value { b'\xf5' } else { b'\xf4' })
    }

    pub fn encode_none(&mut self, py: Python<'_>) -> PyResult<()> {
        self.fp_write_byte(py, b'\xf6')
    }

    pub fn encode_undefined(&mut self, py: Python<'_>) -> PyResult<()> {
        self.fp_write_byte(py, b'\xf7')
    }

    pub fn encode_semantic(
        &mut self,
        py: Python<'_>,
        tag: u64,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let old_string_referencing = self.string_referencing;
        if tag == 256 {
            self.string_referencing = true;
            // TODO: move the string/bytestring references here temporarily
        }
        let mut result = self.encode_length(py, 6, Some(tag));
        if result.is_ok() {
            result = self.encode(py, &value);
        }
        self.string_referencing = old_string_referencing;
        // TODO: restore the string/bytestring references to the instance
        result
    }

    pub fn encode_set(&mut self, py: Python<'_>, value: &Bound<'_, PySet>) -> PyResult<()> {
        // Semantic tag 258
        self.encode_semantic(py, 258, PyTuple::new(py, value)?.as_any())
    }

    //
    // Special encoders (major tag 7)
    //

    fn encode_simple_value(
        &mut self,
        py: Python<'_>,
        obj: &Bound<'_, CBORSimpleValue>,
    ) -> PyResult<()> {
        let value = obj.get().value;
        if value < 24 {
            self.fp_write_byte(py, 0xe0 | value)
        } else {
            self.fp_write_byte(py, 0xf8)?;
            self.fp_write_byte(py, value)
        }
    }

    fn encode_float(&mut self, py: Python<'_>, value: &Bound<'_, PyFloat>) -> PyResult<()> {
        let value = value.extract::<f64>()?;
        if value.is_nan() {
            self.fp_write(py, b"\xf9\x7e\x00".to_vec())
        } else if value.is_infinite() {
            self.fp_write(py, b"\xf9\x7c\x00".to_vec())
        } else {
            self.fp_write_byte(py, 0xfb)?;
            self.fp_write(py, value.to_be_bytes().to_vec())
        }
    }

    fn encode_complex(&mut self, py: Python<'_>, value: &Bound<'_, PyComplex>) -> PyResult<()> {
        let tuple = PyTuple::new(py, [value.real(), value.imag()])?;
        self.encode_semantic(py, 43000, tuple.as_any())
    }

    // def encode_complex(self, value: complex) -> None:
    //     # Semantic tag 43000
    //     with self.disable_value_sharing():
    //         self.encode_semantic(CBORTag(43000, [value.real, value.imag]))

}
