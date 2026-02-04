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
    use pyo3::types::PyDict;
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

    pub static ENCODERS: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
    pub static MAJOR_DECODERS: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
    pub static SEMANTIC_DECODERS: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
    pub static SYS_MAXSIZE: PyOnceLock<usize> = PyOnceLock::new();
    pub static UNDEFINED: PyOnceLock<Py<UndefinedType>> = PyOnceLock::new();
    pub static BREAK_MARKER: PyOnceLock<Py<BreakMarkerType>> = PyOnceLock::new();

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
    ///  :param str_errors:
    ///      determines how to handle unicode decoding errors (see the `Error Handlers`_
    ///      section in the standard library documentation for details)
    ///  :return:
    ///      the deserialized object
    ///
    ///  .. _Error Handlers: https://docs.python.org/3/library/codecs.html#error-handlers
    #[pyfunction]
    #[pyo3(signature = (
        fp: "typing.IO[bytes]",
        /, *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        str_errors: "str" = "strict",
    ))]
    fn load<'py>(
        py: Python<'py>,
        fp: &Bound<'py, PyAny>,
        tag_hook: Option<&Bound<'py, PyAny>>,
        object_hook: Option<&Bound<'py, PyAny>>,
        str_errors: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let decoder = CBORDecoder::new(py, fp, tag_hook, object_hook, str_errors, 4096)?;
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
    ///  :param str_errors:
    ///      determines how to handle unicode decoding errors (see the `Error Handlers`_
    ///      section in the standard library documentation for details)
    ///  :return:
    ///      the deserialized object
    ///
    ///  .. _Error Handlers: https://docs.python.org/3/library/codecs.html#error-handlers
    #[pyfunction]
    #[pyo3(signature = (
        data: "bytes",
        /, *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        str_errors: "str" = "strict"
    ))]
    fn loads<'py>(
        py: Python<'py>,
        data: Vec<u8>,
        tag_hook: Option<&Bound<'py, PyAny>>,
        object_hook: Option<&Bound<'py, PyAny>>,
        str_errors: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let decoder =
            CBORDecoder::new_internal(py, None, data, tag_hook, object_hook, str_errors, 4096)?;
        let instance = Bound::new(py, decoder)?;
        CBORDecoder::decode(&instance)
    }

    ///  Serialize an object to a file.
    ///
    ///  :param fp:
    ///      the file to write to (any file-like object opened for writing in binary mode)
    ///  :param obj:
    ///      the object to serialize
    ///  :param datetime_as_timestamp:
    ///      set to ``True`` to serialize datetimes as UNIX timestamps (this makes datetimes
    ///      more concise on the wire, but loses the timezone information)
    ///  :param timezone:
    ///      the default timezone to use for serializing naive datetimes; if this is not
    ///      specified naive datetimes will throw a :exc:`ValueError` when encoding is
    ///      attempted
    ///  :param value_sharing:
    ///      set to ``True`` to allow more efficient serializing of repeated values
    ///      and, more importantly, cyclic data structures, at the cost of extra
    ///      line overhead
    ///  :param default:
    ///      a callable that is called by the encoder with two arguments (the encoder
    ///      instance and the value being encoded) when no suitable encoder has been found,
    ///      and should use the methods on the encoder to encode any objects it wants to add
    ///      to the data stream
    ///  :param canonical:
    ///      when ``True``, use "canonical" CBOR representation; this typically involves
    ///      sorting maps, sets, etc. into a pre-determined order ensuring that
    ///      serializations are comparable without decoding
    ///  :param date_as_datetime:
    ///      set to ``True`` to serialize date objects as datetimes (CBOR tag 0), which was
    ///      the default behavior in previous releases (cbor2 <= 4.1.2).
    ///  :param string_referencing:
    ///      set to ``True`` to allow more efficient serializing of repeated string values
    ///  :param indefinite_containers:
    ///      encode containers as indefinite (use stop code instead of specifying length)
    ///  :return: the serialized output
    #[pyfunction]
    #[pyo3(signature = (
        obj,
        /,
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
    fn dump<'py>(
        py: Python<'py>,
        obj: &Bound<'py, PyAny>,
        fp: &Bound<'py, PyAny>,
        datetime_as_timestamp: bool,
        timezone: Option<&Bound<'py, PyAny>>,
        value_sharing: bool,
        default: Option<&Bound<'py, PyAny>>,
        canonical: bool,
        date_as_datetime: bool,
        string_referencing: bool,
        indefinite_containers: bool,
    ) -> PyResult<()> {
        let encoder = CBOREncoder::new(
            py,
            Some(fp),
            datetime_as_timestamp,
            timezone,
            value_sharing,
            default,
            canonical,
            date_as_datetime,
            string_referencing,
            indefinite_containers,
        )?;
        let instance = Bound::new(py, encoder)?;
        CBOREncoder::encode(&instance, obj)
    }

    ///  Serialize an object to a bytestring.
    ///
    ///  :param obj:
    ///      the object to serialize
    ///  :param datetime_as_timestamp:
    ///      set to ``True`` to serialize datetimes as UNIX timestamps (this makes datetimes
    ///      more concise on the wire, but loses the timezone information)
    ///  :param timezone:
    ///      the default timezone to use for serializing naive datetimes; if this is not
    ///      specified naive datetimes will throw a :exc:`ValueError` when encoding is
    ///      attempted
    ///  :param value_sharing:
    ///      set to ``True`` to allow more efficient serializing of repeated values
    ///      and, more importantly, cyclic data structures, at the cost of extra
    ///      line overhead
    ///  :param default:
    ///      a callable that is called by the encoder with two arguments (the encoder
    ///      instance and the value being encoded) when no suitable encoder has been found,
    ///      and should use the methods on the encoder to encode any objects it wants to add
    ///      to the data stream
    ///  :param canonical:
    ///      when ``True``, use "canonical" CBOR representation; this typically involves
    ///      sorting maps, sets, etc. into a pre-determined order ensuring that
    ///      serializations are comparable without decoding
    ///  :param date_as_datetime:
    ///      set to ``True`` to serialize date objects as datetimes (CBOR tag 0), which was
    ///      the default behavior in previous releases (cbor2 <= 4.1.2).
    ///  :param string_referencing:
    ///      set to ``True`` to allow more efficient serializing of repeated string values
    ///  :param indefinite_containers:
    ///      encode containers as indefinite (use stop code instead of specifying length)
    ///  :return: the serialized output
    #[pyfunction]
    #[pyo3(signature = (
        obj,
        /, *,
        datetime_as_timestamp: "bool" = false,
        timezone: "datetime.tzinfo | None" = None,
        value_sharing: "bool" = false,
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
        default: Option<&Bound<'_, PyAny>>,
        canonical: bool,
        date_as_datetime: bool,
        string_referencing: bool,
        indefinite_containers: bool,
    ) -> PyResult<Vec<u8>> {
        let encoder = CBOREncoder::new(
            py,
            None,
            datetime_as_timestamp,
            timezone,
            value_sharing,
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
        let cbor_decoder_type = py.get_type::<CBORDecoder>();
        let encoders = PyDict::new(py);
        let major_decoders = PyDict::new(py);
        let semantic_decoders = PyDict::new(py);

        let register_encoder = |class_name: &str, encoder_func_name: &str| -> PyResult<()> {
            let (module_name, class_name) = class_name.rsplit_once('.').ok_or_else(|| {
                PyValueError::new_err(format!("Invalid fully qualified type name: {}", class_name))
            })?;
            let py_type = py.import(module_name)?.getattr(class_name)?;
            encoders.set_item(py_type, cbor_encoder_type.getattr(encoder_func_name)?)
        };

        let register_major_decoder = |tag: u8, decoder_func_name: &str| -> PyResult<()> {
            major_decoders.set_item(tag, cbor_decoder_type.getattr(decoder_func_name)?)
        };

        let register_semantic_decoder = |tag: u64, decoder_func_name: &str| -> PyResult<()> {
            semantic_decoders.set_item(tag, cbor_decoder_type.getattr(decoder_func_name)?)
        };

        // Register encoder callbacks
        register_encoder("builtins.str", "encode_string")?;
        register_encoder("builtins.bytes", "encode_bytes")?;
        register_encoder("builtins.bytearray", "encode_bytearray")?;
        register_encoder("builtins.int", "encode_int")?;
        register_encoder("builtins.float", "encode_float")?;
        register_encoder("builtins.complex", "encode_complex")?;
        register_encoder("builtins.bool", "encode_bool")?;
        register_encoder("decimal.Decimal", "encode_decimal")?;
        register_encoder("datetime.datetime", "encode_datetime")?;
        register_encoder("datetime.date", "encode_date")?;
        register_encoder("fractions.Fraction", "encode_rational")?;
        register_encoder("re.Pattern", "encode_regexp")?;
        register_encoder("email.mime.text.MIMEText", "encode_mime")?;
        register_encoder("uuid.UUID", "encode_uuid")?;
        register_encoder("builtins.set", "encode_set")?;
        register_encoder("builtins.frozenset", "encode_frozenset")?;
        register_encoder("ipaddress.IPv4Address", "encode_ipv4_address")?;
        register_encoder("ipaddress.IPv6Address", "encode_ipv6_address")?;
        register_encoder("ipaddress.IPv4Network", "encode_ipv4_network")?;
        register_encoder("ipaddress.IPv6Network", "encode_ipv6_network")?;
        register_encoder("ipaddress.IPv4Interface", "encode_ipv4_interface")?;
        register_encoder("ipaddress.IPv6Interface", "encode_ipv6_interface")?;
        m.add("encoders", encoders.clone())?;
        ENCODERS.get_or_init(py, || encoders.unbind());

        // Register decoder callbacks for major tags
        register_major_decoder(0, "decode_uint")?;
        register_major_decoder(1, "decode_negint")?;
        register_major_decoder(2, "decode_bytestring")?;
        register_major_decoder(3, "decode_string")?;
        register_major_decoder(4, "decode_array")?;
        register_major_decoder(5, "decode_map")?;
        register_major_decoder(6, "decode_semantic")?;
        register_major_decoder(7, "decode_special")?;
        m.add("major_decoders", major_decoders.clone())?;
        MAJOR_DECODERS.get_or_init(py, || major_decoders.unbind());

        // Register decoder callbacks for semantic tags
        register_semantic_decoder(0, "decode_datetime_string")?;
        register_semantic_decoder(1, "decode_epoch_datetime")?;
        register_semantic_decoder(2, "decode_positive_bignum")?;
        register_semantic_decoder(3, "decode_negative_bignum")?;
        register_semantic_decoder(4, "decode_fraction")?;
        register_semantic_decoder(5, "decode_bigfloat")?;
        register_semantic_decoder(25, "decode_stringref")?;
        register_semantic_decoder(28, "decode_shareable")?;
        register_semantic_decoder(29, "decode_sharedref")?;
        register_semantic_decoder(30, "decode_rational")?;
        register_semantic_decoder(35, "decode_regexp")?;
        register_semantic_decoder(36, "decode_mime")?;
        register_semantic_decoder(37, "decode_uuid")?;
        register_semantic_decoder(52, "decode_ipv4")?;
        register_semantic_decoder(54, "decode_ipv6")?;
        register_semantic_decoder(100, "decode_epoch_date")?;
        register_semantic_decoder(256, "decode_stringref_namespace")?;
        register_semantic_decoder(258, "decode_set")?;
        register_semantic_decoder(260, "decode_ipaddress")?;
        register_semantic_decoder(261, "decode_ipnetwork")?;
        register_semantic_decoder(1004, "decode_date_string")?;
        register_semantic_decoder(43000, "decode_complex")?;
        register_semantic_decoder(55799, "decode_self_describe_cbor")?;
        m.add("semantic_decoders", semantic_decoders.clone())?;
        SEMANTIC_DECODERS.get_or_init(py, || semantic_decoders.unbind());

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
