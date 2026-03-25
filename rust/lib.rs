mod decoder;
mod encoder;
mod types;
mod utils;

use pyo3::prelude::pymodule;

/// A Python module implemented in Rust.
#[pymodule]
mod _cbor2 {
    use pyo3::prelude::*;
    use pyo3::sync::PyOnceLock;
    use pyo3::types::{PyBytes, PyMapping};
    use std::mem::take;

    #[pymodule_export]
    use crate::encoder::CBOREncoder;

    #[pymodule_export]
    use crate::encoder::shareable_encoder;

    #[pymodule_export]
    use crate::decoder::CBORDecoder;

    #[pymodule_export]
    use crate::decoder::shareable_decoder;

    #[pymodule_export]
    use crate::types::CBORTag;

    #[pymodule_export]
    use crate::types::CBORSimpleValue;

    #[cfg(not(Py_3_15))]
    #[pymodule_export]
    use crate::types::FrozenDict;

    use crate::types::UndefinedType;

    #[pymodule_export]
    use crate::types::CBORError;

    #[pymodule_export]
    use crate::types::CBOREncodeError;

    #[pymodule_export]
    use crate::types::CBOREncodeTypeError;

    #[pymodule_export]
    use crate::types::CBOREncodeValueError;

    #[pymodule_export]
    use crate::types::CBORDecodeError;

    #[pymodule_export]
    use crate::types::CBORDecodeEOF;

    pub static SYS_MAXSIZE: PyOnceLock<usize> = PyOnceLock::new();
    pub static UNDEFINED: PyOnceLock<Py<UndefinedType>> = PyOnceLock::new();
    pub static BREAK_MARKER: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

    ///  Deserialize an object from an open file.
    ///
    /// :param fp: the file to read from (any file-like object opened for reading in binary mode)
    /// :param tag_hook:
    ///     callable that takes 2 arguments: the decoder instance, and the :class:`.CBORTag`
    ///     to be decoded. This callback is invoked for any tags for which there is no
    ///     specific decoder. The return value is substituted for the :class:`.CBORTag`
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
    ///     determines how to handle unicode decoding errors (see the `Error Handlers`_
    ///     section in the standard library documentation for details)
    /// :param read_size: minimum amount of bytes to read at once
    ///     (ignored if ``fp`` is not seekable)
    /// :param max_depth:
    ///     maximum allowed depth for nested containers
    /// :param allow_indefinite:
    ///     if :data:`False`, raise a :exc:`CBORDecodeError` when encountering an indefinite-length
    ///     string or container in the input stream
    /// :param immutable:
    ///     if :data:`True`, return immutable objects (e.g. :class:`frozenset` and :class:`tuple`)
    ///     instead of mutable objects (e.g. :class:`list` and :class:`dict`)
    /// :return:
    ///     the deserialized object
    ///
    /// .. _Error Handlers: https://docs.python.org/3/library/codecs.html#error-handlers
    #[pyfunction]
    #[pyo3(signature = (
        fp,
        *,
        tag_hook = None,
        object_hook = None,
        semantic_decoders = None,
        str_errors = "strict",
        read_size = 4096,
        max_depth = 400,
        allow_indefinite = true,
        immutable = false,
    ))]
    fn load<'py>(
        py: Python<'py>,
        fp: &Bound<'py, PyAny>,
        tag_hook: Option<&Bound<'py, PyAny>>,
        object_hook: Option<&Bound<'py, PyAny>>,
        semantic_decoders: Option<&Bound<'py, PyMapping>>,
        str_errors: &str,
        read_size: usize,
        max_depth: usize,
        allow_indefinite: bool,
        immutable: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut decoder = CBORDecoder::new(
            py,
            fp,
            tag_hook,
            object_hook,
            semantic_decoders,
            str_errors,
            read_size,
            max_depth,
            allow_indefinite,
        )?;
        decoder.decode(py, immutable)
    }

    /// Deserialize an object from a bytestring.
    ///
    /// :param data:
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
    /// :param semantic_decoders:
    ///     An optional mapping for overriding the decoding for select semantic tags.
    ///     The value is a mapping of semantic tags (integers) to callables that take
    ///     the decoder instance as the sole argument.
    /// :param str_errors:
    ///     determines how to handle unicode decoding errors (see the `Error Handlers`_
    ///     section in the standard library documentation for details)
    /// :param max_depth:
    ///     maximum allowed depth for nested containers
    /// :param allow_indefinite:
    ///     if :data:`False`, raise a :exc:`CBORDecodeError` when encountering an indefinite-length
    ///     string or container in the input stream
    /// :param immutable:
    ///     if :data:`True`, return immutable objects (e.g. :class:`frozenset` and :class:`tuple`)
    ///     instead of mutable objects (e.g. :class:`list` and :class:`dict`)
    /// :return:
    ///     the deserialized object
    ///
    /// .. _Error Handlers: https://docs.python.org/3/library/codecs.html#error-handlers
    #[pyfunction]
    #[pyo3(signature = (
        data,
        *,
        tag_hook = None,
        object_hook = None,
        semantic_decoders = None,
        str_errors = "strict",
        max_depth = 400,
        allow_indefinite = true,
        immutable = false,
    ))]
    fn loads<'py>(
        py: Python<'py>,
        data: Bound<'py, PyBytes>,
        tag_hook: Option<&Bound<'py, PyAny>>,
        object_hook: Option<&Bound<'py, PyAny>>,
        semantic_decoders: Option<&Bound<'py, PyMapping>>,
        str_errors: &str,
        max_depth: usize,
        allow_indefinite: bool,
        immutable: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut decoder = CBORDecoder::new_internal(
            py,
            None,
            Some(data),
            tag_hook,
            object_hook,
            semantic_decoders,
            str_errors,
            0,
            max_depth,
            allow_indefinite,
        )?;
        decoder.decode(py, immutable)
    }

    /// Serialize an object to a file.
    ///
    /// :param obj:
    ///     the object to serialize
    /// :param fp:
    ///     the file to write to (any file-like object opened for writing in binary mode)
    /// :param datetime_as_timestamp:
    ///     set to ``True`` to serialize datetimes as UNIX timestamps (this makes datetimes
    ///     more concise on the wire, but loses the timezone information)
    /// :param timezone:
    ///     the default timezone to use for serializing naive datetimes; if this is not
    ///     specified naive datetimes will throw a :exc:`ValueError` when encoding is attempted
    /// :param value_sharing:
    ///     set to ``True`` to allow more efficient serializing of repeated values
    ///     and, more importantly, cyclic data structures, at the cost of extra
    ///     line overhead
    /// :param encoders:
    ///     An optional mapping for overriding the encoding for select Python types.
    ///     Each key in this mapping should be a Python type object, and the value a callable
    ///     that takes two arguments: the encoder object and the object to encode.
    /// :param default:
    ///     a callable that is called by the encoder with two arguments (the encoder
    ///     instance and the value being encoded) when no suitable encoder has been found,
    ///     and should use the methods on the encoder to encode any objects it wants to add
    ///     to the data stream
    /// :param canonical:
    ///     when ``True``, use "canonical" CBOR representation; this typically involves
    ///     sorting maps, sets, etc. into a pre-determined order ensuring that
    ///     serializations are comparable without decoding
    /// :param date_as_datetime:
    ///     set to ``True`` to serialize date objects as datetimes (CBOR tag 0), which was
    ///     the default behavior in previous releases (cbor2 <= 4.1.2).
    /// :param string_referencing:
    ///     set to ``True`` to allow more efficient serializing of repeated string values
    /// :param indefinite_containers:
    ///     encode containers as indefinite (use stop code instead of specifying length)
    #[pyfunction]
    #[pyo3(signature = (
        obj,
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
    /// :param datetime_as_timestamp:
    ///     set to ``True`` to serialize datetimes as UNIX timestamps (this makes datetimes
    ///     more concise on the wire, but loses the timezone information)
    /// :param timezone:
    ///     the default timezone to use for serializing naive datetimes; if this is not
    ///     specified naive datetimes will throw a :exc:`ValueError` when encoding is
    ///     attempted
    /// :param value_sharing:
    ///     set to ``True`` to allow more efficient serializing of repeated values
    ///     and, more importantly, cyclic data structures, at the cost of extra
    ///     line overhead
    /// :param encoders:
    ///     An optional mapping for overriding the encoding for select Python types.
    ///     Each key in this mapping should be a Python type object, and the value a callable
    ///     that takes two arguments: the encoder object and the object to encode.
    /// :param default:
    ///     a callable that is called by the encoder with two arguments (the encoder
    ///     instance and the value being encoded) when no suitable encoder has been found,
    ///     and should use the methods on the encoder to encode any objects it wants to add
    ///     to the data stream
    /// :param canonical:
    ///     when ``True``, use "canonical" CBOR representation; this typically involves
    ///     sorting maps, sets, etc. into a pre-determined order ensuring that
    ///     serializations are comparable without decoding
    /// :param date_as_datetime:
    ///     set to ``True`` to serialize date objects as datetimes (CBOR tag 0), which was
    ///     the default behavior in previous releases (cbor2 <= 4.1.2).
    /// :param string_referencing:
    ///     set to ``True`` to allow more efficient serializing of repeated string values
    /// :param indefinite_containers:
    ///     encode containers as indefinite (use stop code instead of specifying length)
    /// :return: the serialized output
    #[pyfunction]
    #[pyo3(signature = (
        obj,
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

        #[cfg(not(Py_3_15))]
        {
            let frozen_dict_type = py.get_type::<FrozenDict>();
            py.import("collections.abc")?
                .getattr("Mapping")?
                .call_method1("register", (frozen_dict_type,))?;
        }

        let undefined = Bound::new(py, UndefinedType)?;
        m.add("undefined", undefined.clone())?;
        UNDEFINED.get_or_init(py, || undefined.unbind());

        BREAK_MARKER.get_or_try_init(py, || {
            py.import("builtins")?
                .getattr("object")?
                .call0()
                .map(Bound::unbind)
        })?;
        SYS_MAXSIZE.get_or_try_init(py, || py.import("sys")?.getattr("maxsize")?.extract())?;

        Ok(())
    }
}
