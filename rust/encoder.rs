use crate::types::{BreakMarkerType, CBORSimpleValue, CBORTag, UndefinedType};
use bigdecimal::{BigDecimal, ToPrimitive};
use half::f16;
use num_bigint::BigInt;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyByteArray, PyBytes, PyComplex, PyFloat, PyFrozenSet, PyInt, PyMapping, PySequence, PySet, PyString, PyTuple};
use pyo3::{pyclass, IntoPyObjectExt, Py, PyAny};
use std::collections::HashMap;

#[pyclass(subclass, module = "cbor2")]
pub struct CBOREncoder {
    pub fp: Py<PyAny>,

    #[pyo3(get)]
    pub datetime_as_timestamp: bool,

    pub timezone: Option<Py<PyAny>>,

    #[pyo3(get)]
    pub value_sharing: bool,

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
        value: &Bound<'_, PyAny>,
        f: impl FnOnce() -> PyResult<()>,
    ) -> PyResult<()> {
        let py = slf.py();
        let value_sharing = slf.borrow().value_sharing;
        let value_id = value.as_ptr() as usize;
        let instance = slf.borrow();
        let option = instance.shared_containers.get(&value_id);
        match option {
            None => {
                drop(instance);
                slf.borrow_mut();
                if value_sharing {
                    // Mark the container as shareable
                    let next_index = PyInt::new(py, slf.borrow().shared_containers.len()).unbind();
                    slf.borrow_mut()
                        .shared_containers
                        .insert(value_id, (value.clone().unbind(), Some(next_index)));
                    f().map(|_| ())
                } else {
                    slf.borrow_mut()
                        .shared_containers
                        .insert(value_id, (value.clone().unbind(), None));
                    let result = f();
                    slf.borrow_mut().shared_containers.remove(&value_id);
                    result.map(|_| ())
                }
            }
            Some((_, None)) => {
                let exc =
                    py.import("cbor2._types")?
                        .getattr("CBOREncodeValueError")?
                        .call1(("cyclic data structure detected but value sharing is disabled",))?;
                Err(PyErr::from_value(exc))
            },
            Some((_, Some(index))) => {
                // Generate a reference to the previous index instead of
                // encoding this again
                let value = index.clone_ref(py);
                drop(instance);
                slf.borrow_mut().encode_length(py, 6, Some(0x1D))?;
                CBOREncoder::encode_int(slf, value.bind(py))
            }
        }
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
        let result = fp.getattr("write");
        if let Ok(write) = result && write.is_callable() {
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

    #[pyo3(signature = (bytes: "bytes", /))]
    pub fn write(&mut self, py: Python<'_>, bytes: Vec<u8>) -> PyResult<()> {
        self.fp.call_method1(py, "write", (&bytes,)).map(|_| ())
    }

    pub fn encode_value(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let decimal_type = slf.py().import("decimal")?.getattr("Decimal")?;
        if obj.is_none() {
            CBOREncoder::encode_none(slf)
        } else if obj.is_exact_instance_of::<UndefinedType>() {
            CBOREncoder::encode_undefined(slf)
        } else if obj.is_exact_instance_of::<BreakMarkerType>() {
            CBOREncoder::encode_break(slf)
        } else if let Ok(simple_value) = obj.cast::<CBORSimpleValue>() {
            CBOREncoder::encode_simple_value(slf, simple_value)
        } else if let Ok(tag) = obj.cast::<CBORTag>() {
            let tag = tag.get();
            CBOREncoder::encode_semantic(slf, tag.tag, tag.value.bind(slf.py()))
        } else if let Ok(string) = obj.cast::<PyString>() {
            CBOREncoder::encode_string(slf, string)
        } else if let Ok(bytes) = obj.cast::<PyBytes>() {
            CBOREncoder::encode_bytes(slf, bytes)
        } else if let Ok(bytearray) = obj.cast::<PyByteArray>() {
            CBOREncoder::encode_bytearray(slf, bytearray)
        } else if let Ok(bool) = obj.cast::<PyBool>() {
            CBOREncoder::encode_bool(slf, bool.is_true())
        } else if let Ok(integer) = obj.cast::<PyInt>() {
            CBOREncoder::encode_int(slf, integer)
        } else if let Ok(integer) = obj.cast::<PyInt>() {
            CBOREncoder::encode_int(slf, integer)
        } else if let Ok(float) = obj.cast::<PyFloat>() {
            CBOREncoder::encode_float(slf, float)
        } else if let Ok(complex) = obj.cast::<PyComplex>() {
            CBOREncoder::encode_complex(slf, complex)
        } else if obj.is_instance(&decimal_type)? {
            CBOREncoder::encode_decimal(slf, obj)
        } else if let Ok(map) = obj.cast::<PyMapping>() {
            CBOREncoder::encode_map(slf, map)
        } else if let Ok(sequence) = obj.cast::<PySequence>() {
            CBOREncoder::encode_array(slf, sequence)
        } else if let Ok(set) = obj.cast::<PySet>() {
            CBOREncoder::encode_set(slf, set)
        } else if let Ok(set) = obj.cast::<PyFrozenSet>() {
            CBOREncoder::encode_frozenset(slf, set)
        } else if let Some(default) = &slf.borrow().default {
            default.call1(slf.py(), (slf, obj)).map(|_| ())
        } else {
            let msg = format!("cannot encode type {}", obj.get_type().to_string());
            let exc = slf.py().import("cbor2._types")?
                .getattr("CBOREncodeError")?
                .call1((msg,))?;
            Err(PyErr::from_value(exc))
        }
    }

    pub fn encode(slf: &Bound<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        CBOREncoder::encode_value(slf, obj)?;
        slf.borrow_mut().flush(slf.py())
    }

    // #[pyo3(signature = (
    //     encoder: "Callable[[CBOREncoder, typing.Any], typing.Any]",
    //     value: "typing.Any"
    // ))]
    // pub fn encode_shared(
    //     slf: &Bound<'_, Self>,
    //     encoder: &Bound<'_, PyAny>,
    //     value: &Bound<'_, PyAny>,
    // ) -> PyResult<()> {
    //     let py = slf.py();
    //     let instance = slf.borrow();
    //     let value_sharing = instance.value_sharing;
    //     let shared_containers = &instance.shared_containers;
    //
    //     let id = py.import("builtins")?.getattr("id")?;
    //     let value_id = id.call1((value,))?.extract::<usize>()?;
    //     match shared_containers.get(&value_id) {
    //         Some((_, index)) => {
    //             match index {
    //                 Some(index) => {
    //                     // Generate a reference to the previous index instead of
    //                     // encoding this again
    //                     slf.borrow_mut().encode_length(py, 6, Some(0x1D))?;
    //                     CBOREncoder::encode_int(slf, index.bind(py))
    //                 }
    //                 None => {
    //                     let error_class =
    //                         py.import("cbor2._types")?.getattr("CBOREncodeValueError")?;
    //                     let error = error_class.call1((
    //                         "cyclic data structure detected but value sharing is disabled",
    //                     ))?;
    //                     Err(PyErr::from_value(error))
    //                 }
    //             }
    //         }
    //         None => {
    //             if value_sharing {
    //                 // Mark the container as shareable
    //                 let next_index = PyInt::new(py, instance.shared_containers.len()).unbind();
    //                 slf.borrow_mut()
    //                     .shared_containers
    //                     .insert(value_id, (value.clone().unbind(), Some(next_index)));
    //                 encoder.call1((slf, value)).map(|_| ())
    //             } else {
    //                 slf.borrow_mut()
    //                     .shared_containers
    //                     .insert(value_id, (value.clone().unbind(), None));
    //                 let result = encoder.call1((slf.clone(), value));
    //                 slf.borrow_mut().shared_containers.remove(&value_id);
    //                 result.map(|_| ())
    //             }
    //         }
    //     }
    // }

    #[pyo3(signature = (major_tag: "int", length: "int | None" = None))]
    pub fn encode_length(
        &mut self,
        py: Python<'_>,
        major_tag: u8,
        length: Option<u64>,
    ) -> PyResult<()> {
        // println!("packing: major_tag={}", major_tag);
        let major_tag = major_tag << 5;
        // println!("packing: major_tag<<5 = {}", major_tag);
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
        slf.borrow_mut().encode_length(py, 2, Some(obj.len() as u64))?;
        slf.borrow_mut().fp_write(py, obj.to_vec())
    }

    fn encode_array(slf: &Bound<'_, Self>, obj: &Bound<'_, PySequence>) -> PyResult<()> {
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
        CBOREncoder::encode_shared_internal(slf, obj, || {
            for value in obj.try_iter()? {
                CBOREncoder::encode_value(slf, &value?)?;
            }
            Ok(())
        })?;

        if indefinite_containers {
            CBOREncoder::encode_break(slf)?;
        }
        Ok(())
    }

    fn encode_map(slf: &Bound<'_, Self>, obj: &Bound<'_, PyMapping>) -> PyResult<()> {
        let indefinite_containers = slf.borrow().indefinite_containers;
        let py = slf.py();
        slf.borrow_mut().encode_length(
            py,
            5,
            if !indefinite_containers {
                Some(obj.len()? as u64)
            } else {
                None
            },
        )?;
        CBOREncoder::encode_shared_internal(slf, obj, || {
            for item in obj.items()?.try_iter()? {
                let (key, value): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item?.extract()?;
                CBOREncoder::encode_value(slf, &key)?;
                CBOREncoder::encode_value(slf, &value)?;
            }
            Ok(())
        })?;

        if indefinite_containers {
            CBOREncoder::encode_break(slf)?
        }
        Ok(())
    }

    pub fn encode_break(slf: &Bound<'_, Self>) -> PyResult<()> {
        // Break stop code for indefinite containers
        slf.borrow_mut().fp_write_byte(slf.py(), 0xff)
    }

    pub fn encode_int(slf: &Bound<'_, Self>, integer: &Bound<'_, PyInt>) -> PyResult<()> {
        let py = slf.py();
        if integer.ge(18446744073709551616_i128)? {
            println!("integer {} is greater or equivalent to {}", integer, 18446744073709551616_i128);
            let (_, payload) = integer.extract::<BigInt>()?.to_bytes_be();
            let py_payload = PyBytes::new(py, &payload);
            CBOREncoder::encode_semantic(slf, 2, py_payload.as_any())
        } else if integer.lt(-18446744073709551616_i128)? {
            println!("integer {} is lower than {}", integer, -18446744073709551616_i128);
            let mut value = integer.extract::<BigInt>()?;
            value = -value - 1;
            let (_, payload) = value .to_bytes_be();
            let py_payload = PyBytes::new(py, &payload);
            CBOREncoder::encode_semantic(slf, 3, py_payload.as_any())
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
            result = CBOREncoder::encode(slf, &value);
        }
        slf.borrow_mut().string_referencing = old_string_referencing;
        // TODO: restore the string/bytestring references to the instance
        result
    }

    pub fn encode_set(slf: &Bound<'_, Self>, value: &Bound<'_, PySet>) -> PyResult<()> {
        // Semantic tag 258
        CBOREncoder::encode_semantic(slf, 258, PyTuple::new(slf.py(), value)?.as_any())
    }

    pub fn encode_frozenset(slf: &Bound<'_, Self>, value: &Bound<'_, PyFrozenSet>) -> PyResult<()> {
        // Semantic tag 258
        CBOREncoder::encode_semantic(slf, 258, PyTuple::new(slf.py(), value)?.as_any())
    }

    //
    // Semantic decoders (major tag 6)
    //

    pub fn encode_decimal(slf: &Bound<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if value.call_method0("is_nan")?.is_truthy()? {
            slf.borrow_mut().fp_write(slf.py(), vec![0xf9, 0x7e, 0x00])
        } else if value.call_method0("is_infinite")?.is_truthy()? {
            let signed = value.call_method0("is_signed")?.is_truthy()?;
            let middle = if signed { 0xfc } else { 0x7c };
            slf.borrow_mut().fp_write(slf.py(), vec![0xf9, middle, 0x00])
        } else {
            let py = slf.py();
            let decimal: BigDecimal = value.extract()?;
            let (digits, exp) = decimal.as_bigint_and_exponent();
            let py_exp = (-exp).into_bound_py_any(py)?;
            let py_digits = digits.into_bound_py_any(py)?;
            let parts = PyTuple::new(py, &[py_exp, py_digits])?;
            CBOREncoder::encode_semantic(slf, 4, &parts)
        }
    }

    // def encode_decimal(self, value: Decimal) -> None:
        // # Semantic tag 4
        // if value.is_nan():
        //     self._fp_write(b"\xf9\x7e\x00")
        // elif value.is_infinite():
        //     self._fp_write(b"\xf9\x7c\x00" if value > 0 else b"\xf9\xfc\x00")
        // else:
        //     dt = value.as_tuple()
        //     sig = 0
        //     for digit in dt.digits:
        //         sig = (sig * 10) + digit
        //     if dt.sign:
        //         sig = -sig
        //     with self.disable_value_sharing():
        //         self.encode_semantic(CBORTag(4, [dt.exponent, sig]))

    //
    // Special encoders (major tag 7)
    //

    fn encode_simple_value(
        slf: &Bound<'_, Self>,
        obj: &Bound<'_, CBORSimpleValue>,
    ) -> PyResult<()> {
        let py = slf.py();
        let value = obj.get().value;
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
                        return slf.borrow_mut().fp_write(py, value_16.to_be_bytes().to_vec())
                    } else {
                        slf.borrow_mut().fp_write_byte(py, 0xfa)?;
                        return slf.borrow_mut().fp_write(py, value_32.to_be_bytes().to_vec())
                    }
                }
            }
            slf.borrow_mut().fp_write_byte(py, 0xfb)?;
            slf.borrow_mut().fp_write(py, value.to_be_bytes().to_vec())
        }
    }

    fn encode_complex(slf: &Bound<'_, Self>, value: &Bound<'_, PyComplex>) -> PyResult<()> {
        let tuple = PyTuple::new(slf.py(), [value.real(), value.imag()])?;
        CBOREncoder::encode_semantic(slf, 43000, tuple.as_any())
    }
}
