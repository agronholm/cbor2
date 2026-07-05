//! The low-level, "streaming" CBOR decoder.
//!
//! [`CBORStreamDecoder`] reads bytes from a file-like object (or an in-memory
//! buffer) and produces a flat stream of primitive [`crate::tokens`] rather than
//! fully assembled Python objects. The high-level [`crate::decoder::CBORDecoder`]
//! consumes this token stream and performs the final assembly.

use crate::tokens;
use crate::types::{CBORDecodeEOF, CBORDecodeError};
use crate::utils::{raise_exc_from, wrap_decode_error};
use half::f16;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};
use pyo3::{IntoPyObjectExt, intern};
use std::ffi::CStr;

pub(crate) const SEEK_CUR: u8 = 1;

/// A single CBOR primitive token, produced while reading the head of a data item.
///
/// This is the internal (Rust-side) representation; it is converted to and from
/// the Python token classes in [`crate::tokens`] only at the Python boundary, so
/// the fast path (``load``/``loads``) never allocates Python token objects.
pub enum Token<'py> {
    Integer(Bound<'py, PyAny>),
    ByteString(Bound<'py, PyBytes>, usize),
    TextString(Bound<'py, PyString>, usize),
    ByteStringStart,
    TextStringStart,
    ArrayStart(Option<usize>),
    MapStart(Option<usize>),
    Tag(u64),
    Simple(u8),
    Float(f64),
    Boolean(bool),
    Null,
    Undefined,
    Break,
}

impl<'py> Token<'py> {
    /// Converts this token into its Python token object representation.
    pub fn into_py(self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Ok(match self {
            Token::Integer(value) => Bound::new(
                py,
                tokens::Integer {
                    value: value.unbind(),
                },
            )?
            .into_any(),
            Token::ByteString(value, length) => Bound::new(
                py,
                tokens::ByteString {
                    value: value.unbind(),
                    length,
                },
            )?
            .into_any(),
            Token::TextString(value, length) => Bound::new(
                py,
                tokens::TextString {
                    value: value.unbind(),
                    length,
                },
            )?
            .into_any(),
            Token::ByteStringStart => Bound::new(py, tokens::ByteStringStart)?.into_any(),
            Token::TextStringStart => Bound::new(py, tokens::TextStringStart)?.into_any(),
            Token::ArrayStart(length) => Bound::new(py, tokens::ArrayStart { length })?.into_any(),
            Token::MapStart(length) => Bound::new(py, tokens::MapStart { length })?.into_any(),
            Token::Tag(number) => Bound::new(py, tokens::Tag { number })?.into_any(),
            Token::Simple(value) => Bound::new(py, tokens::Simple { value })?.into_any(),
            Token::Float(value) => Bound::new(py, tokens::Float { value })?.into_any(),
            Token::Boolean(value) => Bound::new(py, tokens::Boolean { value })?.into_any(),
            Token::Null => Bound::new(py, tokens::Null)?.into_any(),
            Token::Undefined => Bound::new(py, tokens::UndefinedToken)?.into_any(),
            Token::Break => Bound::new(py, tokens::Break)?.into_any(),
        })
    }

    /// Converts a Python token object back into an internal [`Token`].
    pub fn from_py(obj: &Bound<'py, PyAny>) -> PyResult<Token<'py>> {
        let py = obj.py();
        if let Ok(token) = obj.cast::<tokens::Integer>() {
            Ok(Token::Integer(token.get().value.bind(py).clone()))
        } else if let Ok(token) = obj.cast::<tokens::ByteString>() {
            let token = token.get();
            Ok(Token::ByteString(
                token.value.bind(py).clone(),
                token.length,
            ))
        } else if let Ok(token) = obj.cast::<tokens::TextString>() {
            let token = token.get();
            Ok(Token::TextString(
                token.value.bind(py).clone(),
                token.length,
            ))
        } else if obj.cast::<tokens::ByteStringStart>().is_ok() {
            Ok(Token::ByteStringStart)
        } else if obj.cast::<tokens::TextStringStart>().is_ok() {
            Ok(Token::TextStringStart)
        } else if let Ok(token) = obj.cast::<tokens::ArrayStart>() {
            Ok(Token::ArrayStart(token.get().length))
        } else if let Ok(token) = obj.cast::<tokens::MapStart>() {
            Ok(Token::MapStart(token.get().length))
        } else if let Ok(token) = obj.cast::<tokens::Tag>() {
            Ok(Token::Tag(token.get().number))
        } else if let Ok(token) = obj.cast::<tokens::Simple>() {
            Ok(Token::Simple(token.get().value))
        } else if let Ok(token) = obj.cast::<tokens::Float>() {
            Ok(Token::Float(token.get().value))
        } else if let Ok(token) = obj.cast::<tokens::Boolean>() {
            Ok(Token::Boolean(token.get().value))
        } else if obj.cast::<tokens::Null>().is_ok() {
            Ok(Token::Null)
        } else if obj.cast::<tokens::UndefinedToken>().is_ok() {
            Ok(Token::Undefined)
        } else if obj.cast::<tokens::Break>().is_ok() {
            Ok(Token::Break)
        } else {
            Err(PyValueError::new_err(format!(
                "expected a cbor2 token object, got {}",
                obj.get_type().qualname()?
            )))
        }
    }
}

/// The CBORStreamDecoder reads CBOR data and yields a stream of primitive
/// :mod:`tokens <cbor2.tokens>` describing the head of each data item, without
/// assembling containers or interpreting semantic tags. It is the low-level
/// building block used by :class:`~cbor2.CBORDecoder`.
///
/// :param fp: the file to read from (any file-like object opened for reading in binary mode)
/// :param str_errors:
///     determines how to handle Unicode decoding errors (see the `Error Handlers`_
///     section in the standard library documentation for details)
/// :param read_size: minimum number of bytes to read at once
///     (ignored if ``fp`` is not seekable)
///
/// .. _Error Handlers: https://docs.python.org/3/library/codecs.html#error-handlers
#[pyclass(module = "cbor2")]
pub struct CBORStreamDecoder {
    fp: Option<Py<PyAny>>,
    str_errors: Option<Py<PyString>>,
    #[pyo3(get)]
    pub(crate) read_size: usize,

    read_method: Option<Py<PyAny>>,
    buffer: Option<Py<PyBytes>>,
    read_position: usize,
    available_bytes: usize,
    fp_is_seekable: bool,
}

impl CBORStreamDecoder {
    pub fn new_internal(
        py: Python<'_>,
        fp: Option<&Bound<'_, PyAny>>,
        buffer: Option<Bound<PyBytes>>,
        str_errors: &str,
        read_size: usize,
    ) -> PyResult<Self> {
        let available_bytes = if let Some(buffer) = buffer.as_ref() {
            buffer.len()?
        } else {
            0
        };
        let bound_str_errors = PyString::new(py, str_errors);
        let mut this = Self {
            fp: None,
            str_errors: None,
            read_size,
            read_method: None,
            buffer: buffer.map(Bound::unbind),
            read_position: 0,
            available_bytes,
            fp_is_seekable: false,
        };
        if let Some(fp) = fp {
            this.set_fp(fp)?
        };
        this.set_str_errors(&bound_str_errors)?;
        Ok(this)
    }

    /// Performs a single read from the underlying file object. Returns
    /// ``Ok(None)`` if the read returned zero bytes (clean EOF).
    fn read_from_fp_raw<'py>(
        &mut self,
        py: Python<'py>,
        bytes_to_read: usize,
    ) -> PyResult<Option<(Bound<'py, PyBytes>, usize)>> {
        let Some(read) = self.read_method.as_ref() else {
            return Ok(None);
        };
        let bytes_from_fp: Bound<PyBytes> = read.bind(py).call1((bytes_to_read,))?.cast_into()?;
        let num_read_bytes = bytes_from_fp.len()?;
        if num_read_bytes == 0 {
            Ok(None)
        } else {
            Ok(Some((bytes_from_fp, num_read_bytes)))
        }
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
        let num_read_bytes = match self.read_from_fp_raw(py, bytes_to_read)? {
            Some((bytes_from_fp, num_read_bytes)) if num_read_bytes >= minimum_amount => {
                return Ok((bytes_from_fp, num_read_bytes));
            }
            Some((_, num_read_bytes)) => num_read_bytes,
            None => 0,
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
            if self.read_position > 0 {
                concatenated_buffer.drain(..self.read_position);
            }
            concatenated_buffer.truncate(self.available_bytes);
            let (new_bytes, amount_read) = self.read_from_fp(py, needed_bytes)?;
            concatenated_buffer.extend_from_slice(&new_bytes[..needed_bytes]);
            self.buffer = Some(new_bytes.unbind());
            self.available_bytes = amount_read - needed_bytes;
            self.read_position = needed_bytes;
            Ok(concatenated_buffer
                .try_into()
                .expect("buffer size mismatch"))
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

    fn read_bytes(&mut self, py: Python<'_>, amount: usize) -> PyResult<Vec<u8>> {
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
                self.buffer.take().unwrap().as_bytes(py)[self.read_position..].to_vec();
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

    /// Reads a single initial byte of a data item. Returns ``Ok(None)`` when the
    /// stream is cleanly exhausted at an item boundary (only if ``allow_eof``).
    fn read_initial_byte(&mut self, py: Python<'_>, allow_eof: bool) -> PyResult<Option<u8>> {
        if self.available_bytes >= 1 {
            let byte = self.buffer.as_ref().unwrap().bind(py).as_bytes()[self.read_position];
            self.available_bytes -= 1;
            self.read_position += 1;
            return Ok(Some(byte));
        }

        let read_size: usize = if self.fp_is_seekable {
            self.read_size
        } else {
            1
        };
        match self.read_from_fp_raw(py, read_size.max(1))? {
            Some((new_bytes, amount_read)) => {
                let byte = new_bytes.as_bytes()[0];
                self.read_position = 1;
                self.available_bytes = amount_read - 1;
                self.buffer = Some(new_bytes.unbind());
                Ok(Some(byte))
            }
            None if allow_eof => Ok(None),
            None => Err(CBORDecodeEOF::new_err(
                "premature end of stream (expected to read at least 1 bytes, got 0 instead)",
            )),
        }
    }

    /// Decode the length of the next item.
    ///
    /// :return: the length of the item, or :data:`None` to indicate an indefinite-length item
    fn decode_length(&mut self, py: Python<'_>, subtype: u8) -> PyResult<Option<u64>> {
        let length = match subtype {
            ..24 => Some(subtype as u64),
            24 => Some(self.read_exact::<1>(py)?[0] as u64),
            25 => Some(u16::from_be_bytes(self.read_exact(py)?) as u64),
            26 => Some(u32::from_be_bytes(self.read_exact(py)?) as u64),
            27 => Some(u64::from_be_bytes(self.read_exact(py)?)),
            // Indefinite length; whether it is permitted is a policy decision left
            // to the high-level decoder.
            31 => None,
            _ => {
                return Err(CBORDecodeError::new_err(format!(
                    "unknown unsigned integer subtype 0x{subtype:x}"
                )));
            }
        };
        Ok(length)
    }

    fn decode_length_finite(&mut self, py: Python<'_>, subtype: u8) -> PyResult<u64> {
        match self.decode_length(py, subtype)? {
            Some(length) => Ok(length),
            None => Err(CBORDecodeError::new_err(
                "indefinite length not allowed here",
            )),
        }
    }

    fn decode_length_as_usize(&mut self, py: Python<'_>, subtype: u8) -> PyResult<Option<usize>> {
        match self.decode_length(py, subtype)? {
            Some(length) => usize::try_from(length).map(Some).map_err(|_| {
                CBORDecodeError::new_err(format!(
                    "huge item length {length} exceeds the system address space"
                ))
            }),
            None => Ok(None),
        }
    }

    /// Reads ``length`` bytes into a :class:`bytes` object, in chunks of at most
    /// 64 KiB so that a truncated payload claiming a huge length cannot force a
    /// large up-front allocation.
    fn read_string_bytes<'py>(
        &mut self,
        py: Python<'py>,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        PyBytes::new_with_writer(py, length.min(65536), |writer| {
            let mut remaining_length = length;
            while remaining_length > 0 {
                let chunk_size = remaining_length.min(65536);
                let chunk = self.read_bytes(py, chunk_size)?;
                remaining_length -= chunk_size;
                writer.write_all(&chunk)?;
            }
            Ok(())
        })
    }

    fn str_errors_cstr(&self, py: Python<'_>) -> PyResult<Option<&'static CStr>> {
        match self.str_errors.as_ref() {
            None => Ok(None),
            // set_str_errors only ever stores these values
            Some(str_errors) => Ok(Some(match str_errors.to_str(py)? {
                "ignore" => c"ignore",
                "replace" => c"replace",
                "backslashreplace" => c"backslashreplace",
                "surrogateescape" => c"surrogateescape",
                other => {
                    return Err(CBORDecodeError::new_err(format!(
                        "invalid str_errors value: '{other}'"
                    )));
                }
            })),
        }
    }

    /// Reads and UTF-8 decodes ``length`` bytes into a :class:`str`.
    fn read_text<'py>(&mut self, py: Python<'py>, length: usize) -> PyResult<Bound<'py, PyString>> {
        // Fast path for strings that fit within a single read: decode straight
        // into a str without allocating an intermediate bytes object.
        if length <= 65536 {
            let bytes = self.read_bytes(py, length)?;
            return match self.str_errors.as_ref() {
                None => PyString::from_bytes(py, &bytes),
                Some(str_errors) => bytes
                    .into_bound_py_any(py)?
                    .call_method1(
                        intern!(py, "decode"),
                        (intern!(py, "utf-8"), str_errors.bind(py)),
                    )?
                    .cast_into()
                    .map_err(PyErr::from),
            };
        }

        // Large strings are read in 64 KiB chunks into a single bytes object,
        // then decoded in one pass (keeping multi-byte characters that straddle
        // a chunk boundary intact).
        let bytes = self.read_string_bytes(py, length)?;
        let errors = self.str_errors_cstr(py)?;
        PyString::from_encoded_object(bytes.as_any(), Some(c"utf-8"), errors)
            .and_then(|s| s.cast_into().map_err(PyErr::from))
    }

    /// Reads the head of the next data item and returns it as a [`Token`].
    ///
    /// Returns ``Ok(None)`` only when ``allow_eof`` is set and the stream is
    /// cleanly exhausted at an item boundary.
    pub fn next_token<'py>(
        &mut self,
        py: Python<'py>,
        allow_eof: bool,
    ) -> PyResult<Option<Token<'py>>> {
        let Some(initial_byte) = self.read_initial_byte(py, allow_eof)? else {
            return Ok(None);
        };
        let major_type = initial_byte >> 5;
        let subtype = initial_byte & 31;
        let typename = major_type_name(major_type);
        self.decode_item(py, major_type, subtype)
            .map(Some)
            .map_err(|err| wrap_decode_error(py, err, &typename))
    }

    fn decode_item<'py>(
        &mut self,
        py: Python<'py>,
        major_type: u8,
        subtype: u8,
    ) -> PyResult<Token<'py>> {
        match major_type {
            0 => {
                let uint = self.decode_length_finite(py, subtype)?;
                Ok(Token::Integer(uint.into_bound_py_any(py)?))
            }
            1 => {
                let uint = self.decode_length_finite(py, subtype)?;
                let signed_int = -(uint as i128) - 1;
                Ok(Token::Integer(signed_int.into_bound_py_any(py)?))
            }
            2 => match self.decode_length_as_usize(py, subtype)? {
                None => Ok(Token::ByteStringStart),
                Some(length) => {
                    let bytes = self.read_string_bytes(py, length)?;
                    Ok(Token::ByteString(bytes, length))
                }
            },
            3 => match self.decode_length_as_usize(py, subtype)? {
                None => Ok(Token::TextStringStart),
                Some(length) => {
                    let text = self.read_text(py, length)?;
                    Ok(Token::TextString(text, length))
                }
            },
            4 => Ok(Token::ArrayStart(self.decode_length_as_usize(py, subtype)?)),
            5 => Ok(Token::MapStart(self.decode_length_as_usize(py, subtype)?)),
            6 => Ok(Token::Tag(self.decode_length_finite(py, subtype)?)),
            7 => self.decode_special(py, subtype),
            _ => Err(CBORDecodeError::new_err(format!(
                "invalid major type: {major_type}"
            ))),
        }
    }

    fn decode_special<'py>(&mut self, py: Python<'py>, subtype: u8) -> PyResult<Token<'py>> {
        match subtype {
            0..20 => Ok(Token::Simple(subtype)),
            20 => Ok(Token::Boolean(false)),
            21 => Ok(Token::Boolean(true)),
            22 => Ok(Token::Null),
            23 => Ok(Token::Undefined),
            24 => {
                let value = self.read_exact::<1>(py)?[0];
                if value < 0x20 {
                    return Err(CBORDecodeError::new_err(
                        "invalid two-byte sequence for simple value",
                    ));
                }
                Ok(Token::Simple(value))
            }
            25 => {
                let bytes = self.read_exact::<2>(py)?;
                Ok(Token::Float(f16::from_be_bytes(bytes).to_f64()))
            }
            26 => {
                let bytes = self.read_exact::<4>(py)?;
                Ok(Token::Float(f32::from_be_bytes(bytes) as f64))
            }
            27 => {
                let bytes = self.read_exact::<8>(py)?;
                Ok(Token::Float(f64::from_be_bytes(bytes)))
            }
            31 => Ok(Token::Break),
            _ => Err(CBORDecodeError::new_err(format!(
                "undefined reserved major type 7 subtype 0x{subtype:x}"
            ))),
        }
    }

    /// If the underlying file object is seekable and bytes have been read ahead
    /// into the buffer, rewind the file to the position immediately following the
    /// last consumed byte and drop the buffer.
    pub fn rewind_buffer(&mut self, py: Python<'_>) -> PyResult<()> {
        if self.available_bytes > 0
            && let Some(fp) = &self.fp
        {
            let offset = -(self.available_bytes as isize);
            fp.call_method1(py, intern!(py, "seek"), (offset, SEEK_CUR))?;
            self.buffer = None;
            self.available_bytes = 0;
            self.read_position = 0;
        }
        Ok(())
    }
}

#[pymethods]
impl CBORStreamDecoder {
    #[new]
    #[pyo3(signature = (fp, *, str_errors = "strict", read_size = 4096))]
    pub fn new(
        py: Python<'_>,
        fp: &Bound<'_, PyAny>,
        str_errors: &str,
        read_size: usize,
    ) -> PyResult<Self> {
        Self::new_internal(py, Some(fp), None, str_errors, read_size)
    }

    #[getter]
    pub fn fp(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.fp.as_ref().map(|fp| fp.clone_ref(py))
    }

    #[setter]
    pub fn set_fp(&mut self, fp: &Bound<'_, PyAny>) -> PyResult<()> {
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
    pub fn str_errors(&self, py: Python<'_>) -> Py<PyString> {
        if let Some(str_errors) = self.str_errors.as_ref() {
            str_errors.clone_ref(py)
        } else {
            intern!(py, "strict").clone().unbind()
        }
    }

    #[setter]
    pub fn set_str_errors(&mut self, str_errors: &Bound<'_, PyString>) -> PyResult<()> {
        let as_string: &str = str_errors.extract()?;
        self.str_errors = match as_string {
            "strict" => None,
            "ignore" | "replace" | "backslashreplace" | "surrogateescape" => {
                Some(str_errors.clone().unbind())
            }
            _ => {
                return Err(PyValueError::new_err(format!(
                    "invalid str_errors value: '{str_errors}'"
                )));
            }
        };
        Ok(())
    }

    /// Read bytes from the data stream.
    ///
    /// :param amount: the number of bytes to read
    #[pyo3(signature = (amount, /))]
    pub fn read(&mut self, py: Python<'_>, amount: usize) -> PyResult<Vec<u8>> {
        self.read_bytes(py, amount)
    }

    /// Decode and return the next primitive token from the stream.
    ///
    /// :raises CBORDecodeEOF: if the stream ends in the middle of a data item
    /// :return: the next :mod:`token <cbor2.tokens>`
    fn decode_token<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match self.next_token(py, false)? {
            Some(token) => token.into_py(py),
            None => Err(CBORDecodeEOF::new_err(
                "premature end of stream (expected to read at least 1 bytes, got 0 instead)",
            )),
        }
    }

    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    /// Return the next token, raising :exc:`StopIteration` at a clean
    /// end-of-stream (i.e. an item boundary), or :exc:`CBORDecodeEOF` if the
    /// stream ends in the middle of a data item.
    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        match self.next_token(py, true)? {
            Some(token) => token.into_py(py).map(Some),
            None => Ok(None),
        }
    }
}

pub(crate) fn major_type_name(major_type: u8) -> &'static str {
    match major_type {
        0 => "unsigned integer",
        1 => "negative integer",
        2 => "byte string",
        3 => "text string",
        4 => "array",
        5 => "map",
        6 => "semantic tag",
        7 => "special value",
        _ => "value",
    }
}
