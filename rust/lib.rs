mod decoder;
mod encoder;
mod types;
mod utils;

use pyo3::prelude::pymodule;

/// A Python module implemented in Rust.
#[pymodule]
mod _cbor2 {
    use pyo3::exceptions::PyValueError;
    use pyo3::prelude::*;
    use pyo3::sync::PyOnceLock;
    use pyo3::types::{PyBytes, PyDict, PyMapping};
    use std::mem::take;

    #[pymodule_export]
    use crate::encoder::CBOREncoder;

    #[pymodule_export]
    use crate::encoder::shareable_encoder;

    #[pymodule_export]
    use crate::decoder::CBORDecoder;

    #[pymodule_export]
    use crate::types::CBORTag;

    #[pymodule_export]
    use crate::types::CBORSimpleValue;

    #[pymodule_export]
    use crate::types::FrozenDict;

    use crate::types::BreakMarkerType;
    use crate::types::UndefinedType;

    pub static SYS_MAXSIZE: PyOnceLock<usize> = PyOnceLock::new();
    pub static UNDEFINED: PyOnceLock<Py<UndefinedType>> = PyOnceLock::new();
    pub static BREAK_MARKER: PyOnceLock<Py<BreakMarkerType>> = PyOnceLock::new();

    pub const DEFAULT_READ_SIZE: usize = 4096;
    #[cfg(PyPy)]
    pub const DEFAULT_MAX_DEPTH: usize = 200;  // PyPy segfaults at much larger nesting depth
    #[cfg(not(PyPy))]
    pub const DEFAULT_MAX_DEPTH: usize = 950;

    ///  Deserialize an object from a bytestring.
    ///
    ///  :param bytes s:
    ///      the bytestring to deserialize
    ///  :param tag_hook:
    ///      callable that takes 2 arguments: the decoder instance, and the :class:`.CBORTag`
    ///      to be decoded. This callback is invoked for any tags for which there is no
    ///      specific decoder. The return value is substituted for the :class:`.CBORTag`
    ///      object in the deserialized output
    ///  :param object_hook:
    ///      callable that takes 2 arguments: the decoder instance, and a dictionary. This
    ///      callback is invoked for each deserialized :class:`dict` object. The return value
    ///      is substituted for the dict in the deserialized output.
    ///  :param major_decoders:
    ///      An optional mapping for overriding the decoders for select major types.
    ///      The value is a mapping of major types (integers 0-7) to callable that take 2
    ///      arguments: the decoder instance and a numeric subtype.
    ///  :param semantic_decoders:
    ///      An optional mapping for overriding the decoding for select semantic tags.
    ///      The value is a mapping of semantic tags (integers) to callables that take
    ///      the decoder instance as the sole argument.
    ///  :param str_errors:
    ///      determines how to handle unicode decoding errors (see the `Error Handlers`_
    ///      section in the standard library documentation for details)
    ///  :param int max_depth:
    ///      maximum allowed depth for nested containers
    ///  :return:
    ///      the deserialized object
    ///
    ///  .. _Error Handlers: https://docs.python.org/3/library/codecs.html#error-handlers
    #[pyfunction]
    #[pyo3(signature = (
        fp: "typing.IO[bytes]",
        *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        major_decoders = None,
        semantic_decoders = None,
        str_errors: "str" = "strict",
        max_depth: "int" = DEFAULT_MAX_DEPTH,
    ))]
    fn load<'py>(
        py: Python<'py>,
        fp: &Bound<'py, PyAny>,
        tag_hook: Option<&Bound<'py, PyAny>>,
        object_hook: Option<&Bound<'py, PyAny>>,
        major_decoders: Option<&Bound<'py, PyMapping>>,
        semantic_decoders: Option<&Bound<'py, PyMapping>>,
        str_errors: &str,
        max_depth: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let decoder = CBORDecoder::new(
            py,
            fp,
            tag_hook,
            object_hook,
            major_decoders,
            semantic_decoders,
            str_errors,
            DEFAULT_READ_SIZE,
            max_depth,
        )?;
        let instance = Bound::new(py, decoder)?;
        CBORDecoder::decode(&instance)
    }

    ///  Deserialize an object from a bytestring.
    ///
    ///  :param bytes data:
    ///      the bytestring to deserialize
    ///  :param tag_hook:
    ///      callable that takes 2 arguments: the decoder instance, and the :class:`.CBORTag`
    ///      to be decoded. This callback is invoked for any tags for which there is no
    ///      built-in decoder. The return value is substituted for the :class:`.CBORTag`
    ///      object in the deserialized output
    ///  :param object_hook:
    ///      callable that takes 2 arguments: the decoder instance, and a dictionary. This
    ///      callback is invoked for each deserialized :class:`dict` object. The return value
    ///      is substituted for the dict in the deserialized output.
    ///  :param major_decoders:
    ///      An optional mapping for overriding the decoders for select major types.
    ///      The value is a mapping of major types (integers 0-7) to callable that take 2
    ///      arguments: the decoder instance and a numeric subtype.
    ///  :param semantic_decoders:
    ///      An optional mapping for overriding the decoding for select semantic tags.
    ///      The value is a mapping of semantic tags (integers) to callables that take
    ///      the decoder instance as the sole argument.
    ///  :param str_errors:
    ///      determines how to handle unicode decoding errors (see the `Error Handlers`_
    ///      section in the standard library documentation for details)
    ///  :param int max_depth:
    ///      maximum allowed depth for nested containers
    ///  :return:
    ///      the deserialized object
    ///
    ///  .. _Error Handlers: https://docs.python.org/3/library/codecs.html#error-handlers
    #[pyfunction]
    #[pyo3(signature = (
        data: "bytes",
        *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        major_decoders = None,
        semantic_decoders = None,
        str_errors: "str" = "strict",
        max_depth: "int" = DEFAULT_MAX_DEPTH,
    ))]
    fn loads<'py>(
        py: Python<'py>,
        data: Bound<'py, PyBytes>,
        tag_hook: Option<&Bound<'py, PyAny>>,
        object_hook: Option<&Bound<'py, PyAny>>,
        major_decoders: Option<&Bound<'py, PyMapping>>,
        semantic_decoders: Option<&Bound<'py, PyMapping>>,
        str_errors: &str,
        max_depth: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let decoder = CBORDecoder::new_internal(
            py,
            None,
            Some(data),
            tag_hook,
            object_hook,
            major_decoders,
            semantic_decoders,
            str_errors,
            DEFAULT_READ_SIZE,
            max_depth,
        )?;
        let instance = Bound::new(py, decoder)?;
        CBORDecoder::decode(&instance)
    }

    /// Serialize an object to a file.
    ///
    /// :param obj:
    ///     the object to serialize
    /// :param ~typing.IO[bytes] fp:
    ///     the file to write to (any file-like object opened for writing in binary mode)
    /// :param bool datetime_as_timestamp:
    ///     set to ``True`` to serialize datetimes as UNIX timestamps (this makes datetimes
    ///     more concise on the wire, but loses the timezone information)
    /// :param ~datetime.tzinfo timezone:
    ///     the default timezone to use for serializing naive datetimes; if this is not
    ///     specified naive datetimes will throw a :exc:`ValueError` when encoding is
    ///     attempted
    /// :param bool value_sharing:
    ///     set to ``True`` to allow more efficient serializing of repeated values
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
    ///     when ``True``, use "canonical" CBOR representation; this typically involves
    ///     sorting maps, sets, etc. into a pre-determined order ensuring that
    ///     serializations are comparable without decoding
    /// :param bool date_as_datetime:
    ///     set to ``True`` to serialize date objects as datetimes (CBOR tag 0), which was
    ///     the default behavior in previous releases (cbor2 <= 4.1.2).
    /// :param bool string_referencing:
    ///     set to ``True`` to allow more efficient serializing of repeated string values
    /// :param bool indefinite_containers:
    ///     encode containers as indefinite (use stop code instead of specifying length)
    /// :rtype: None
    #[pyfunction]
    #[pyo3(signature = (
        obj,
        fp: "typing.IO[bytes]",
        *,
        datetime_as_timestamp: "bool" = false,
        timezone: "datetime.tzinfo | None" = None,
        value_sharing: "bool" = false,
        encoders = None,
        default: "collections.abc.Callable[[CBOREncoder, typing.Any], None] | None" = None,
        canonical: "bool" = false,
        date_as_datetime: "bool" = false,
        string_referencing: "bool" = false,
        indefinite_containers: "bool" = false
    ))]
    fn dump<'py>(
        py: Python<'py>,
        obj: &Bound<'py, PyAny>,
        fp: &Bound<'py, PyAny>,
        datetime_as_timestamp: bool,
        timezone: Option<&Bound<'py, PyAny>>,
        value_sharing: bool,
        encoders: Option<&Bound<'py, PyMapping>>,
        default: Option<&Bound<'py, PyAny>>,
        canonical: bool,
        date_as_datetime: bool,
        string_referencing: bool,
        indefinite_containers: bool,
    ) -> PyResult<()> {
        let encoder = CBOREncoder::new(
            fp,
            datetime_as_timestamp,
            timezone,
            value_sharing,
            encoders,
            default,
            canonical,
            date_as_datetime,
            string_referencing,
            indefinite_containers,
        )?;
        let instance = Bound::new(py, encoder)?;
        CBOREncoder::encode(&instance, obj)
    }

    /// Serialize an object to a bytestring.
    ///
    /// :param obj:
    ///     the object to serialize
    /// :param bool datetime_as_timestamp:
    ///     set to ``True`` to serialize datetimes as UNIX timestamps (this makes datetimes
    ///     more concise on the wire, but loses the timezone information)
    /// :param ~datetime.tzinfo | None timezone:
    ///     the default timezone to use for serializing naive datetimes; if this is not
    ///     specified naive datetimes will throw a :exc:`ValueError` when encoding is
    ///     attempted
    /// :param bool value_sharing:
    ///     set to ``True`` to allow more efficient serializing of repeated values
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
    ///     when ``True``, use "canonical" CBOR representation; this typically involves
    ///     sorting maps, sets, etc. into a pre-determined order ensuring that
    ///     serializations are comparable without decoding
    /// :param bool date_as_datetime:
    ///     set to ``True`` to serialize date objects as datetimes (CBOR tag 0), which was
    ///     the default behavior in previous releases (cbor2 <= 4.1.2).
    /// :param bool string_referencing:
    ///     set to ``True`` to allow more efficient serializing of repeated string values
    /// :param bool indefinite_containers:
    ///     encode containers as indefinite (use stop code instead of specifying length)
    /// :rtype: bytes
    /// :return: the serialized output
    #[pyfunction]
    #[pyo3(signature = (
        obj,
        *,
        datetime_as_timestamp: "bool" = false,
        timezone: "datetime.tzinfo | None" = None,
        value_sharing: "bool" = false,
        encoders = None,
        default: "collections.abc.Callable[[CBOREncoder, typing.Any], None] | None" = None,
        canonical: "bool" = false,
        date_as_datetime: "bool" = false,
        string_referencing: "bool" = false,
        indefinite_containers: "bool" = false
    ))]
    fn dumps<'py>(
        py: Python<'py>,
        obj: &Bound<'py, PyAny>,
        datetime_as_timestamp: bool,
        timezone: Option<&Bound<'_, PyAny>>,
        value_sharing: bool,
        encoders: Option<&Bound<'py, PyMapping>>,
        default: Option<&Bound<'_, PyAny>>,
        canonical: bool,
        date_as_datetime: bool,
        string_referencing: bool,
        indefinite_containers: bool,
    ) -> PyResult<Vec<u8>> {
        let encoder = CBOREncoder::new_internal(
            None,
            datetime_as_timestamp,
            timezone,
            value_sharing,
            encoders,
            default,
            canonical,
            date_as_datetime,
            string_referencing,
            indefinite_containers,
        )?;
        let instance = Bound::new(py, encoder)?;
        CBOREncoder::encode(&instance, obj)?;
        Ok(take(&mut instance.borrow_mut().buffer))
    }

    #[pymodule_init]
    fn init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        // Register cbor2.FrozenDict as a Mapping subclass
        let py = m.py();
        let frozen_dict_type = py.get_type::<FrozenDict>();
        py.import("collections.abc")?
            .getattr("Mapping")?
            .call_method1("register", (frozen_dict_type,))?;

        let cbor_encoder_type = py.get_type::<CBOREncoder>();
        let encoders = PyDict::new(py);

        let register_encoder = |class_name: &str, encoder_func_name: &str| -> PyResult<()> {
            let (module_name, class_name) = class_name.rsplit_once('.').ok_or_else(|| {
                PyValueError::new_err(format!("Invalid fully qualified type name: {}", class_name))
            })?;
            let py_type = py.import(module_name)?.getattr(class_name)?;
            encoders.set_item(py_type, cbor_encoder_type.getattr(encoder_func_name)?)
        };

        let undefined = Bound::new(py, UndefinedType)?;
        m.add("undefined", undefined.clone())?;
        UNDEFINED.get_or_init(py, || undefined.unbind());

        let break_marker = Bound::new(py, BreakMarkerType)?;
        m.add("break_marker", break_marker.clone())?;
        BREAK_MARKER.get_or_init(py, || break_marker.unbind());

        SYS_MAXSIZE.get_or_try_init(py, || py.import("sys")?.getattr("maxsize")?.extract())?;

        Ok(())
    }
}
