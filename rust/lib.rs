mod decoder;
mod encoder;
mod types;

use pyo3::prelude::pymodule;

/// A Python module implemented in Rust.
#[pymodule]
mod _cbor2 {
    use pyo3::exceptions::PyValueError;
    use pyo3::prelude::*;
    use pyo3::types::{PyBytes, PyDict, PyType};

    #[pymodule_export]
    use crate::encoder::CBOREncoder;

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

    ///  Deserialize an object from a bytestring.
    ///
    ///  :param bytes s:
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
        fp: "typing.IO[bytes]", /, *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        str_errors: "str" = "strict",
    ))]
    fn load(
        py: Python<'_>,
        fp: &Bound<'_, PyAny>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        str_errors: &str,
    ) -> PyResult<Py<PyAny>> {
        let decoder = CBORDecoder::new(Some(fp), tag_hook, object_hook, str_errors)?;
        decoder.decode(py)
    }

    ///  Deserialize an object from a bytestring.
    ///
    ///  :param bytes s:
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
        data: "bytes", /, *,
        tag_hook: "collections.abc.Callable | None" = None,
        object_hook: "collections.abc.Callable | None" = None,
        str_errors: "str" = "strict"
    ))]
    fn loads(
        py: Python<'_>,
        data: Vec<u8>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        str_errors: &str,
    ) -> PyResult<Py<PyAny>> {
        let fp = py.import("io")?.getattr("BytesIO")?.call1((data,))?;
        load(py, &fp, tag_hook, object_hook, str_errors)
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
            fp,
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
        obj, /, *,
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
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytesio = py.import("io")?.getattr("BytesIO")?;
        let fp = bytesio.call0()?;
        dump(
            py,
            obj,
            &fp,
            datetime_as_timestamp,
            timezone,
            value_sharing,
            default,
            canonical,
            date_as_datetime,
            string_referencing,
            indefinite_containers,
        )?;
        Ok(fp.call_method0("getvalue")?.cast_into::<PyBytes>()?)
    }

    fn add_encoder(
        encoders: &Bound<'_, PyDict>,
        cbor_encoder_type: &Bound<'_, PyType>,
        class_fqdn: &str,
        encoder_func_name: &str,
    ) -> PyResult<()> {
        if let Some((module_name, class_name)) = class_fqdn.rsplit_once('.') {
            let py_type = encoders.py().import(module_name)?.getattr(class_name)?;
            encoders.set_item(py_type, cbor_encoder_type.getattr(encoder_func_name)?)
        } else {
            Err(PyValueError::new_err(format!(
                "Invalid fully qualified type name: {}",
                class_fqdn
            )))
        }
    }

    #[pymodule_init]
    fn init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        // Register cbor2.FrozenDict as a Mapping subclass
        let py = m.py();
        let frozen_dict_type = py.get_type::<FrozenDict>();
        py.import("collections.abc")?
            .getattr("Mapping")?
            .call_method1("register", (frozen_dict_type,))?;

        // Add the singleton special objects
        m.add("undefined", UndefinedType)?;
        m.add("break_marker", BreakMarkerType)?;

        // Register encoder callbacks
        let cbor_encoder_type = py.get_type::<CBOREncoder>();
        let encoders = PyDict::new(py);
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "decimal.Decimal",
            "encode_decimal",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "datetime.datetime",
            "encode_datetime",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "datetime.date",
            "encode_date",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "fractions.Fraction",
            "encode_rational",
        )?;
        add_encoder(&encoders, &cbor_encoder_type, "re.Pattern", "encode_regexp")?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "email.mime.text.MIMEText",
            "encode_mime",
        )?;
        add_encoder(&encoders, &cbor_encoder_type, "uuid.UUID", "encode_uuid")?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "ipaddress.IPv4Address",
            "encode_ipv4_address",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "ipaddress.IPv6Address",
            "encode_ipv6_address",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "ipaddress.IPv4Network",
            "encode_ipv4_network",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "ipaddress.IPv6Network",
            "encode_ipv6_network",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "ipaddress.IPv4Interface",
            "encode_ipv4_interface",
        )?;
        add_encoder(
            &encoders,
            &cbor_encoder_type,
            "ipaddress.IPv6Interface",
            "encode_ipv6_interface",
        )?;
        m.add("encoders", encoders)?;

        // Register decoder callbacks
        let decoders = PyDict::new(py);
        m.add("decoders", decoders)?;

        Ok(())
    }
}
