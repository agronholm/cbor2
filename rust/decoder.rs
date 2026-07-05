//! The high-level CBOR decoder.
//!
//! [`CBORDecoder`] consumes the primitive token stream produced by
//! [`crate::stream_decoder::CBORStreamDecoder`] and assembles it into fully
//! formed Python objects, applying semantic-tag decoders, hooks, shared/string
//! references, ``immutable`` handling and depth limits.
//!
//! The assembler is a plain-data stack machine (see [`Frame`]) rather than a
//! closure-based one, so its state can be suspended between calls. This enables
//! both the one-shot [`CBORDecoder::decode`] fast path and the incremental
//! [`CBORDecoder::push`] API.

use crate::_cbor2::{BREAK_MARKER, UNDEFINED};
use crate::stream_decoder::{CBORStreamDecoder, Token};
use crate::tokens;
#[cfg(not(Py_3_15))]
use crate::types::FrozenDict;
use crate::types::{
    CBORDecodeError, CBORSimpleValue, CBORTag, DECIMAL_TYPE, FRACTION_TYPE, IPV4ADDRESS_TYPE,
    IPV4INTERFACE_TYPE, IPV4NETWORK_TYPE, IPV6ADDRESS_TYPE, IPV6INTERFACE_TYPE, IPV6NETWORK_TYPE,
    UUID_TYPE,
};
use crate::utils::{PyImportable, create_exc_from, wrap_decode_error};
use pyo3::exceptions::{PyLookupError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyBytes, PyCFunction, PyComplex, PyDict, PyFrozenSet, PyInt, PyList, PyListMethods, PyMapping,
    PySet, PyString, PyTuple,
};
use pyo3::{IntoPyObjectExt, intern};
use std::fmt::{Display, Formatter};

const IMMUTABLE_ATTR: &str = "_cbor2_immutable";
const NAME_ATTR: &str = "_cbor2_name";

static DATE_FROMISOFORMAT: PyImportable = PyImportable::new("datetime", "date.fromisoformat");
static DATE_FROMORDINAL: PyImportable = PyImportable::new("datetime", "date.fromordinal");
static DATETIME_FROMISOFORMAT: PyImportable =
    PyImportable::new("datetime", "datetime.fromisoformat");
static DATETIME_FROMTIMESTAMP: PyImportable =
    PyImportable::new("datetime", "datetime.fromtimestamp");
static EMAIL_PARSER: PyImportable = PyImportable::new("email.parser", "Parser");
static INT_FROMBYTES: PyImportable = PyImportable::new("builtins", "int.from_bytes");
static IPADDRESS_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_address");
static IPNETWORK_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_network");
static IPINTERFACE_FUNC: PyImportable = PyImportable::new("ipaddress", "ip_interface");
static RE_COMPILE: PyImportable = PyImportable::new("re", "compile");
static UTC: PyImportable = PyImportable::new("datetime", "timezone.utc");
#[cfg(Py_3_15)]
static FROZEN_DICT: PyImportable = PyImportable::new("builtins", "frozendict");

/// A transform applied to the (single) decoded content of a semantic tag.
type TransformFn = for<'py> fn(Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>>;

//
// Token hooks: per-token-type callbacks invoked from the native decode loop.
//
// These let callers customize the decoding of specific *leaf* (value-producing)
// tokens while everything else keeps running at full Rust speed. The loop only
// crosses into Python for the token kinds that have a registered hook.
//

const HOOK_INTEGER: usize = 0;
const HOOK_FLOAT: usize = 1;
const HOOK_BOOLEAN: usize = 2;
const HOOK_NULL: usize = 3;
const HOOK_UNDEFINED: usize = 4;
const HOOK_SIMPLE: usize = 5;
const HOOK_BYTESTRING: usize = 6;
const HOOK_TEXTSTRING: usize = 7;
const NUM_TOKEN_HOOKS: usize = 8;

/// Returns the token-hook slot index for a leaf (value-producing) token, or
/// [`None`] for structural tokens (container/string starts, tags, break) which
/// cannot be customized via token hooks.
fn leaf_hook_kind(token: &Token<'_>) -> Option<usize> {
    Some(match token {
        Token::Integer(_) => HOOK_INTEGER,
        Token::Float(_) => HOOK_FLOAT,
        Token::Boolean(_) => HOOK_BOOLEAN,
        Token::Null => HOOK_NULL,
        Token::Undefined => HOOK_UNDEFINED,
        Token::Simple(_) => HOOK_SIMPLE,
        Token::ByteString(..) => HOOK_BYTESTRING,
        Token::TextString(..) => HOOK_TEXTSTRING,
        _ => return None,
    })
}

fn leaf_hook_name(kind: usize) -> &'static str {
    match kind {
        HOOK_INTEGER => "integer",
        HOOK_FLOAT => "float",
        HOOK_BOOLEAN => "boolean",
        HOOK_NULL => "null",
        HOOK_UNDEFINED => "undefined",
        HOOK_SIMPLE => "simple value",
        HOOK_BYTESTRING => "byte string",
        HOOK_TEXTSTRING => "text string",
        _ => "value",
    }
}

/// Maps a token *class* object to its hook slot index.
fn token_hook_kind_index(ty: &Bound<'_, PyAny>) -> PyResult<usize> {
    let py = ty.py();
    let candidates: [(Bound<'_, PyAny>, usize); NUM_TOKEN_HOOKS] = [
        (py.get_type::<tokens::Integer>().into_any(), HOOK_INTEGER),
        (py.get_type::<tokens::Float>().into_any(), HOOK_FLOAT),
        (py.get_type::<tokens::Boolean>().into_any(), HOOK_BOOLEAN),
        (py.get_type::<tokens::Null>().into_any(), HOOK_NULL),
        (
            py.get_type::<tokens::UndefinedToken>().into_any(),
            HOOK_UNDEFINED,
        ),
        (py.get_type::<tokens::Simple>().into_any(), HOOK_SIMPLE),
        (
            py.get_type::<tokens::ByteString>().into_any(),
            HOOK_BYTESTRING,
        ),
        (
            py.get_type::<tokens::TextString>().into_any(),
            HOOK_TEXTSTRING,
        ),
    ];
    for (candidate, index) in candidates {
        if ty.is(&candidate) {
            return Ok(index);
        }
    }
    Err(PyTypeError::new_err(format!(
        "{ty} is not a hookable token type (expected one of the leaf token \
         classes from cbor2.tokens, e.g. Integer, TextString, ByteString)"
    )))
}

/// A human-readable name for the item currently being assembled, used when
/// wrapping errors in a [`CBORDecodeError`].
#[derive(Clone)]
enum DisplayName {
    String(&'static str),
    SemanticTag(u64),
    PythonName(String),
}

impl Display for DisplayName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayName::String(s) => f.write_str(s),
            DisplayName::SemanticTag(tagnum) => write!(f, "semantic tag {}", tagnum),
            DisplayName::PythonName(name) => f.write_str(name),
        }
    }
}

/// Decorates a function to be a two-stage decoder.
///
/// :param name: the name displayed in a :exc:`CBORDecodeError` raised by the decoder
///     (e.g. "error decoding thingamajig") where name='thingamajig`)
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

fn require_tuple<'py>(value: Bound<'py, PyAny>, length: usize) -> PyResult<Bound<'py, PyTuple>> {
    let array: Bound<'py, PyTuple> = value
        .cast_into()
        .map_err(|_| PyTypeError::new_err("input value must be an array"))?;
    if array.len() != length {
        return Err(PyValueError::new_err(format!(
            "expected an array with exactly {length} elements"
        )));
    }
    Ok(array)
}

//
// Assembler stack machine
//

enum ArrayStorage {
    List(Py<PyList>),
    Tuple(Vec<Py<PyAny>>),
}

enum MapStorage {
    Dict(Py<PyDict>),
    Items(Vec<(Py<PyAny>, Py<PyAny>)>),
}

enum FrameKind {
    Array {
        storage: ArrayStorage,
        remaining: Option<usize>,
    },
    Map {
        storage: MapStorage,
        pending_key: Option<Py<PyAny>>,
        remaining: Option<usize>,
        seen_keys: Option<Py<PySet>>,
        map_immutable: bool,
    },
    Set {
        set: Option<Py<PySet>>,
        set_immutable: bool,
    },
    ByteStringChunks(Vec<Py<PyBytes>>),
    TextStringChunks(Vec<Py<PyString>>),
    BuiltinTag(TransformFn),
    UserSemantic(Py<PyAny>),
    ShareablePhase2(Py<PyAny>),
    TagHook {
        tag: Py<PyAny>,
        tag_immutable: bool,
    },
    StringRef,
    SharedRef,
    Shareable {
        index: usize,
    },
    StringNamespace,
}

struct Frame {
    immutable: bool,
    typename: DisplayName,
    kind: FrameKind,
}

/// The working state of the assembler for a single top-level decode.
struct AssemblerState {
    frames: Vec<Frame>,
    shareables: Vec<Option<Py<PyAny>>>,
    string_namespaces: Vec<Vec<Py<PyAny>>>,
    pending_value: Option<Py<PyAny>>,
    current_immutable: bool,
    top_level_immutable: bool,
}

impl AssemblerState {
    fn new(immutable: bool) -> Self {
        Self {
            frames: Vec::new(),
            shareables: Vec::new(),
            string_namespaces: Vec::new(),
            pending_value: None,
            current_immutable: immutable,
            top_level_immutable: immutable,
        }
    }
}

/// The result of feeding a value to the innermost frame.
enum Action<'py> {
    /// The frame needs another item; the flag forces it to be immutable.
    Continue(bool),
    /// The frame is complete with the given value.
    Complete(Bound<'py, PyAny>),
    /// Register the value as shareable ``index`` (unless already set), then complete.
    CompleteShareable(usize, Bound<'py, PyAny>),
    /// Resolve the value (a string reference index) against the active namespace.
    ResolveStringRef(Bound<'py, PyAny>),
    /// Resolve the value (a shared reference index) against the shareables.
    ResolveSharedRef(Bound<'py, PyAny>),
    /// Pop the innermost string namespace, then complete with the value.
    CompletePopNamespace(Bound<'py, PyAny>),
}

/// The CBORDecoder class implements a fully featured `CBOR`_ decoder with
/// several extensions for handling shared references, big integers, rational
/// numbers and so on. Typically, the class is not used directly, but the
/// :func:`load` and :func:`loads` functions are called to indirectly construct
/// and use the class.
///
/// When the class is constructed manually, the main entry point is :meth:`decode`.
///
/// The underlying low-level token stream is available as :attr:`stream`, and
/// tokens may be fed into the decoder one at a time with :meth:`push`.
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
/// :param allow_indefinite:
///     if :data:`False`, raise a :exc:`CBORDecodeError` when encountering an indefinite-length
///     string or container in the input stream
/// :param allow_duplicate_keys:
///     if :data:`False`, raise a :exc:`CBORDecodeError` when a map key that has already been
///     decoded in the same map is encountered
///
/// .. _CBOR: https://cbor.io/
#[pyclass(module = "cbor2")]
pub struct CBORDecoder {
    // The stream decoder is held inline as a plain Rust value so that the fast
    // path (`load`/`loads`) drives it with direct Rust calls, without allocating
    // a second Python object or taking a runtime borrow. It is promoted to a
    // shared `Py<CBORStreamDecoder>` lazily, the first time `stream` is accessed.
    reader: Option<CBORStreamDecoder>,
    stream: Option<Py<CBORStreamDecoder>>,
    tag_hook: Option<Py<PyAny>>,
    object_hook: Option<Py<PyAny>>,
    semantic_decoders: Option<Py<PyMapping>>,
    // Per-token-type hooks, indexed by leaf-token kind. `any_token_hooks` is a
    // cheap flag so the hot loop can skip the lookup entirely when none are set.
    token_hooks: [Option<Py<PyAny>>; NUM_TOKEN_HOOKS],
    token_hooks_map: Option<Py<PyMapping>>,
    any_token_hooks: bool,
    #[pyo3(get)]
    max_depth: usize,
    #[pyo3(get)]
    allow_duplicate_keys: bool,
    // Policy: whether indefinite-length strings/containers are accepted. This is
    // a high-level decoding concern, so it is enforced here rather than in the
    // token stream (which always emits the corresponding "start" tokens).
    #[pyo3(get)]
    allow_indefinite: bool,
}

impl CBORDecoder {
    #[allow(clippy::too_many_arguments)]
    pub fn new_internal(
        py: Python<'_>,
        fp: Option<&Bound<'_, PyAny>>,
        buffer: Option<Bound<PyBytes>>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        semantic_decoders: Option<&Bound<'_, PyMapping>>,
        token_hooks: Option<&Bound<'_, PyMapping>>,
        str_errors: &str,
        read_size: usize,
        max_depth: usize,
        allow_indefinite: bool,
        allow_duplicate_keys: bool,
    ) -> PyResult<Self> {
        let reader = CBORStreamDecoder::new_internal(py, fp, buffer, str_errors, read_size)?;
        let mut this = Self {
            reader: Some(reader),
            stream: None,
            tag_hook: None,
            object_hook: None,
            semantic_decoders: semantic_decoders.map(|d| d.clone().unbind()),
            token_hooks: std::array::from_fn(|_| None),
            token_hooks_map: None,
            any_token_hooks: false,
            max_depth,
            allow_duplicate_keys,
            allow_indefinite,
        };
        this.set_tag_hook(tag_hook)?;
        this.set_object_hook(object_hook)?;
        this.set_token_hooks(token_hooks)?;
        Ok(this)
    }

    /// Runs `f` with shared access to the stream decoder, whether it is still
    /// held inline or has been promoted to a Python object.
    fn with_reader<R>(&self, py: Python<'_>, f: impl FnOnce(&CBORStreamDecoder) -> R) -> R {
        match &self.reader {
            Some(reader) => f(reader),
            None => f(&self.stream.as_ref().unwrap().bind(py).borrow()),
        }
    }

    /// Runs `f` with mutable access to the stream decoder.
    fn with_reader_mut<R>(
        &mut self,
        py: Python<'_>,
        f: impl FnOnce(&mut CBORStreamDecoder) -> R,
    ) -> R {
        match &mut self.reader {
            Some(reader) => f(reader),
            None => f(&mut self.stream.as_ref().unwrap().bind(py).borrow_mut()),
        }
    }

    //
    // Immutability tracking helpers (mirroring the previous closure machine)
    //

    fn begin_frame(state: &mut AssemblerState, requested_immutable: bool) {
        state.current_immutable = state.current_immutable || requested_immutable;
    }

    fn continue_frame(state: &mut AssemblerState, require_immutable: bool) {
        let base = if state.frames.len() >= 2 {
            state.frames[state.frames.len() - 2].immutable
        } else {
            state.top_level_immutable
        };
        state.current_immutable = base || require_immutable;
        state.frames.last_mut().unwrap().immutable = state.current_immutable;
    }

    fn after_pop(state: &mut AssemblerState) {
        state.current_immutable = state
            .frames
            .last()
            .map_or(state.top_level_immutable, |frame| frame.immutable);
    }

    /// Pushes a frame, enforcing ``max_depth`` and performing early
    /// registration of a shareable container.
    fn push_frame(
        &self,
        state: &mut AssemblerState,
        frame: Frame,
        container: Option<Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        if let Some(container) = &container
            && let Some(top) = state.frames.last()
            && let FrameKind::Shareable { index } = top.kind
        {
            state.shareables[index] = Some(container.clone().unbind());
            state.frames.pop();
        }

        if state.frames.len() >= self.max_depth {
            return Err(CBORDecodeError::new_err(format!(
                "maximum container nesting depth ({}) exceeded",
                self.max_depth
            )));
        }

        state.frames.push(frame);
        Ok(())
    }

    fn push_single_child(
        &self,
        state: &mut AssemblerState,
        kind: FrameKind,
        typename: DisplayName,
        requested_immutable: bool,
        container: Option<Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        Self::begin_frame(state, requested_immutable);
        let frame = Frame {
            immutable: state.current_immutable,
            typename,
            kind,
        };
        self.push_frame(state, frame, container)
    }

    /// Hands a leaf token to its registered Python hook and uses the returned
    /// value as the decoded result.
    fn apply_token_hook<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        kind: usize,
        token: Token<'py>,
    ) -> PyResult<()> {
        // Preserve string-reference bookkeeping for the (original) string even
        // when its decoded value is being customized.
        match &token {
            Token::ByteString(value, length) => Self::track_string(state, value.as_any(), *length),
            Token::TextString(value, length) => Self::track_string(state, value.as_any(), *length),
            _ => {}
        }

        let hook = self.token_hooks[kind].as_ref().unwrap().bind(py);
        let token_obj = token.into_py(py)?;
        let value = hook
            .call1((token_obj,))
            .map_err(|e| wrap_decode_error(py, e, &DisplayName::String(leaf_hook_name(kind))))?;
        state.pending_value = Some(value.unbind());
        Ok(())
    }

    /// Enforces the ``allow_indefinite`` policy for indefinite-length items.
    fn check_indefinite_allowed(&self) -> PyResult<()> {
        if self.allow_indefinite {
            Ok(())
        } else {
            Err(CBORDecodeError::new_err(
                "encountered indefinite length but it has been disabled",
            ))
        }
    }

    fn maybe_object_hook<'py>(
        &self,
        py: Python<'py>,
        dict: Bound<'py, PyAny>,
        immutable: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        if let Some(object_hook) = &self.object_hook {
            object_hook.bind(py).call1((dict, immutable))
        } else {
            Ok(dict)
        }
    }

    //
    // Token processing
    //

    /// Processes a freshly obtained token against the current assembler state.
    fn process_token<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        token: Token<'py>,
    ) -> PyResult<()> {
        // String chunk frames intercept the raw token stream.
        if let Some(frame) = state.frames.last() {
            match frame.kind {
                FrameKind::ByteStringChunks(_) => return self.feed_byte_chunk(py, state, token),
                FrameKind::TextStringChunks(_) => return self.feed_text_chunk(py, state, token),
                _ => {}
            }
        }

        // Per-token-type hooks. The `any_token_hooks` flag keeps this a single
        // boolean test on the fast path; only leaf tokens whose kind has a
        // registered hook are handed to Python.
        if self.any_token_hooks
            && let Some(kind) = leaf_hook_kind(&token)
            && self.token_hooks[kind].is_some()
        {
            return self.apply_token_hook(py, state, kind, token);
        }

        match token {
            Token::Break => {
                // An indefinite-length array or map terminates on break; anywhere
                // else the break is treated as an ordinary marker value (matching
                // the reference decoder), which typically leads to a later error.
                let closes_container = matches!(
                    state.frames.last().map(|frame| &frame.kind),
                    Some(FrameKind::Array {
                        remaining: None,
                        ..
                    }) | Some(FrameKind::Map {
                        remaining: None,
                        ..
                    })
                );
                if closes_container {
                    self.handle_break(py, state)
                } else {
                    state.pending_value = Some(BREAK_MARKER.get(py).unwrap().clone_ref(py));
                    Ok(())
                }
            }
            Token::Integer(value) => {
                state.pending_value = Some(value.unbind());
                Ok(())
            }
            Token::Float(value) => {
                state.pending_value = Some(value.into_bound_py_any(py)?.unbind());
                Ok(())
            }
            Token::Boolean(value) => {
                state.pending_value = Some(value.into_bound_py_any(py)?.unbind());
                Ok(())
            }
            Token::Null => {
                state.pending_value = Some(py.None());
                Ok(())
            }
            Token::Undefined => {
                state.pending_value = Some(
                    UNDEFINED
                        .get(py)
                        .unwrap()
                        .bind(py)
                        .clone()
                        .into_any()
                        .unbind(),
                );
                Ok(())
            }
            Token::Simple(value) => {
                let simple = CBORSimpleValue::new(value.into_pyobject(py)?)?;
                state.pending_value = Some(Bound::new(py, simple)?.into_any().unbind());
                Ok(())
            }
            Token::ByteString(value, length) => {
                Self::track_string(state, value.as_any(), length);
                state.pending_value = Some(value.into_any().unbind());
                Ok(())
            }
            Token::TextString(value, length) => {
                Self::track_string(state, value.as_any(), length);
                state.pending_value = Some(value.into_any().unbind());
                Ok(())
            }
            Token::ByteStringStart => {
                self.check_indefinite_allowed()?;
                let frame = Frame {
                    immutable: state.current_immutable,
                    typename: DisplayName::String("byte string"),
                    kind: FrameKind::ByteStringChunks(Vec::new()),
                };
                self.push_frame(state, frame, None)
            }
            Token::TextStringStart => {
                self.check_indefinite_allowed()?;
                let frame = Frame {
                    immutable: state.current_immutable,
                    typename: DisplayName::String("text string"),
                    kind: FrameKind::TextStringChunks(Vec::new()),
                };
                self.push_frame(state, frame, None)
            }
            Token::ArrayStart(length) => {
                if length.is_none() {
                    self.check_indefinite_allowed()?;
                }
                self.handle_array_start(py, state, length)
            }
            Token::MapStart(length) => {
                if length.is_none() {
                    self.check_indefinite_allowed()?;
                }
                self.handle_map_start(py, state, length)
            }
            Token::Tag(tagnum) => self.handle_tag(py, state, tagnum),
        }
    }

    /// Conditionally records a decoded string in the innermost string namespace,
    /// using the same size heuristic as the encoder.
    fn track_string(state: &mut AssemblerState, value: &Bound<'_, PyAny>, length: usize) {
        if let Some(namespace) = state.string_namespaces.last_mut()
            && match namespace.len() {
                0..24 => length >= 3,
                24..256 => length >= 4,
                256..65536 => length >= 5,
                65536..=4294967295 => length >= 7,
                _ => length >= 11,
            }
        {
            namespace.push(value.clone().unbind());
        }
    }

    fn feed_byte_chunk<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        token: Token<'py>,
    ) -> PyResult<()> {
        match token {
            Token::ByteString(value, _) => {
                if let FrameKind::ByteStringChunks(parts) =
                    &mut state.frames.last_mut().unwrap().kind
                {
                    parts.push(value.unbind());
                }
                Ok(())
            }
            Token::Break => {
                let FrameKind::ByteStringChunks(parts) = &state.frames.last().unwrap().kind else {
                    unreachable!()
                };
                let total: usize = parts.iter().map(|p| p.bind(py).as_bytes().len()).sum();
                let joined = PyBytes::new_with_writer(py, total, |writer| {
                    for part in parts {
                        writer.write_all(part.bind(py).as_bytes())?;
                    }
                    Ok(())
                })?;
                state.frames.pop();
                Self::after_pop(state);
                state.pending_value = Some(joined.into_any().unbind());
                Ok(())
            }
            other => Err(CBORDecodeError::new_err(format!(
                "non-byte string (major type {}) found in indefinite length byte string",
                token_major_type(&other)
            ))),
        }
    }

    fn feed_text_chunk<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        token: Token<'py>,
    ) -> PyResult<()> {
        match token {
            Token::TextString(value, _) => {
                if let FrameKind::TextStringChunks(parts) =
                    &mut state.frames.last_mut().unwrap().kind
                {
                    parts.push(value.unbind());
                }
                Ok(())
            }
            Token::Break => {
                let FrameKind::TextStringChunks(parts) = &state.frames.last().unwrap().kind else {
                    unreachable!()
                };
                let list = PyList::new(py, parts.iter().map(|p| p.bind(py)))?;
                let joined = PyString::new(py, "").call_method1(intern!(py, "join"), (list,))?;
                state.frames.pop();
                Self::after_pop(state);
                state.pending_value = Some(joined.unbind());
                Ok(())
            }
            other => Err(CBORDecodeError::new_err(format!(
                "non-text string (major type {}) found in indefinite length text string",
                token_major_type(&other)
            ))),
        }
    }

    fn handle_break<'py>(&self, py: Python<'py>, state: &mut AssemblerState) -> PyResult<()> {
        let built = match state.frames.last() {
            Some(Frame {
                kind:
                    FrameKind::Array {
                        storage,
                        remaining: None,
                    },
                ..
            }) => Self::build_array(py, storage),
            Some(Frame {
                kind:
                    FrameKind::Map {
                        storage,
                        remaining: None,
                        map_immutable,
                        ..
                    },
                ..
            }) => self.build_map(py, storage, *map_immutable)?,
            _ => return Err(CBORDecodeError::new_err("unexpected break code")),
        };
        state.frames.pop();
        Self::after_pop(state);
        state.pending_value = Some(built.unbind());
        Ok(())
    }

    fn build_array<'py>(py: Python<'py>, storage: &ArrayStorage) -> Bound<'py, PyAny> {
        match storage {
            ArrayStorage::List(list) => list.bind(py).clone().into_any(),
            ArrayStorage::Tuple(items) => PyTuple::new(py, items.iter().map(|p| p.bind(py)))
                .unwrap()
                .into_any(),
        }
    }

    fn build_map<'py>(
        &self,
        py: Python<'py>,
        storage: &MapStorage,
        map_immutable: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let container = match storage {
            MapStorage::Dict(dict) => dict.bind(py).clone().into_any(),
            MapStorage::Items(items) => {
                let bound: Vec<(Bound<'py, PyAny>, Bound<'py, PyAny>)> = items
                    .iter()
                    .map(|(k, v)| (k.bind(py).clone(), v.bind(py).clone()))
                    .collect();
                create_frozen_dict(py, bound)?
            }
        };
        self.maybe_object_hook(py, container, map_immutable)
    }

    fn handle_array_start<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        length: Option<usize>,
    ) -> PyResult<()> {
        let immutable = state.current_immutable;
        if length == Some(0) {
            let value = if immutable {
                PyTuple::empty(py).into_any()
            } else {
                PyList::empty(py).into_any()
            };
            state.pending_value = Some(value.unbind());
            return Ok(());
        }

        Self::begin_frame(state, false);
        let (storage, container) = if immutable {
            (ArrayStorage::Tuple(Vec::new()), None)
        } else {
            let list = PyList::empty(py);
            (
                ArrayStorage::List(list.clone().unbind()),
                Some(list.into_any()),
            )
        };
        let frame = Frame {
            immutable: state.current_immutable,
            typename: DisplayName::String("array"),
            kind: FrameKind::Array {
                storage,
                remaining: length,
            },
        };
        self.push_frame(state, frame, container)
    }

    fn handle_map_start<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        length: Option<usize>,
    ) -> PyResult<()> {
        let immutable = state.current_immutable;
        if length == Some(0) {
            let container = if immutable {
                create_frozen_dict(py, Vec::new())?
            } else {
                PyDict::new(py).into_any()
            };
            let value = self
                .maybe_object_hook(py, container, immutable)
                .map_err(|e| wrap_decode_error(py, e, &DisplayName::String("map")))?;
            state.pending_value = Some(value.unbind());
            return Ok(());
        }

        Self::begin_frame(state, true);
        let (storage, container, seen_keys) = if immutable {
            let seen = if self.allow_duplicate_keys {
                None
            } else {
                Some(PySet::empty(py)?.unbind())
            };
            (MapStorage::Items(Vec::new()), None, seen)
        } else {
            let dict = PyDict::new(py);
            (
                MapStorage::Dict(dict.clone().unbind()),
                Some(dict.into_any()),
                None,
            )
        };
        let frame = Frame {
            immutable: state.current_immutable,
            typename: DisplayName::String("map"),
            kind: FrameKind::Map {
                storage,
                pending_key: None,
                remaining: length,
                seen_keys,
                map_immutable: immutable,
            },
        };
        self.push_frame(state, frame, container)
    }

    fn handle_tag<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        tagnum: u64,
    ) -> PyResult<()> {
        if let Some(semantic_decoders) = &self.semantic_decoders {
            match semantic_decoders.bind(py).get_item(tagnum) {
                Ok(decoder) => return self.dispatch_user_decoder(py, state, tagnum, decoder),
                Err(e) if e.is_instance_of::<PyLookupError>(py) => {}
                Err(e) => return Err(e),
            }
        }
        self.dispatch_builtin_tag(py, state, tagnum)
    }

    fn dispatch_user_decoder<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        tagnum: u64,
        decoder: Bound<'py, PyAny>,
    ) -> PyResult<()> {
        let name_attr = decoder.getattr_opt(intern!(py, NAME_ATTR))?;
        if let Some(name) = name_attr {
            // Decorated with @shareable_decoder (two-phase decoder).
            let require_immutable: bool = decoder
                .getattr_opt(intern!(py, IMMUTABLE_ATTR))?
                .map(|x| x.is_truthy())
                .transpose()?
                .unwrap_or(false);
            let retval = decoder.call1((state.current_immutable,))?;
            let tuple: Bound<'py, PyTuple> = retval.cast_into()?;
            if tuple.len() != 2 {
                return Err(CBORDecodeError::new_err(format!(
                    "{decoder} returned a tuple of {} items, expected 2",
                    tuple.len()
                )));
            }
            let container = tuple.get_item(0)?;
            let callback = tuple.get_item(1)?;
            let typename = if name.is_none() {
                DisplayName::SemanticTag(tagnum)
            } else {
                DisplayName::PythonName(name.str()?.to_string())
            };
            let container_opt = if container.is_none() {
                None
            } else {
                Some(container)
            };
            self.push_single_child(
                state,
                FrameKind::ShareablePhase2(callback.unbind()),
                typename,
                require_immutable,
                container_opt,
            )
        } else {
            self.push_single_child(
                state,
                FrameKind::UserSemantic(decoder.unbind()),
                DisplayName::SemanticTag(tagnum),
                state.current_immutable,
                None,
            )
        }
    }

    fn dispatch_builtin_tag<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        tagnum: u64,
    ) -> PyResult<()> {
        let (transform, typename): (TransformFn, &'static str) = match tagnum {
            0 => (Self::decode_datetime_string, "string-form datetime"),
            1 => (Self::decode_epoch_datetime, "epoch-form datetime"),
            2 => (Self::decode_positive_bignum, "positive bignum"),
            3 => (Self::decode_negative_bignum, "negative bignum"),
            4 => (Self::decode_fraction, "decimal fraction"),
            5 => (Self::decode_bigfloat, "bigfloat"),
            25 => {
                return self.push_single_child(
                    state,
                    FrameKind::StringRef,
                    DisplayName::String("string reference"),
                    true,
                    None,
                );
            }
            28 => {
                let index = state.shareables.len();
                state.shareables.push(None);
                let frame = Frame {
                    immutable: state.current_immutable,
                    typename: DisplayName::String("shareable value"),
                    kind: FrameKind::Shareable { index },
                };
                return self.push_frame(state, frame, None);
            }
            29 => {
                return self.push_single_child(
                    state,
                    FrameKind::SharedRef,
                    DisplayName::String("shared reference"),
                    true,
                    None,
                );
            }
            30 => (Self::decode_rational, "rational"),
            35 => (Self::decode_regexp, "regular expression"),
            36 => (Self::decode_mime, "MIME message"),
            37 => (Self::decode_uuid, "UUID"),
            52 => (Self::decode_ipv4, "IPv4 address"),
            54 => (Self::decode_ipv6, "IPv6 address"),
            100 => (Self::decode_epoch_date, "epoch-form date"),
            256 => {
                state.string_namespaces.push(Vec::new());
                let frame = Frame {
                    immutable: state.current_immutable,
                    typename: DisplayName::String("string namespace"),
                    kind: FrameKind::StringNamespace,
                };
                return self.push_frame(state, frame, None);
            }
            258 => {
                let set_immutable = state.current_immutable;
                let set = if set_immutable {
                    None
                } else {
                    Some(PySet::empty(py)?)
                };
                let container = set.as_ref().map(|s| s.clone().into_any());
                return self.push_single_child(
                    state,
                    FrameKind::Set {
                        set: set.map(Bound::unbind),
                        set_immutable,
                    },
                    DisplayName::String("set"),
                    true,
                    container,
                );
            }
            260 => (Self::decode_ipaddress, "IP address"),
            261 => (Self::decode_ipnetwork, "IP network"),
            1004 => (Self::decode_date_string, "string-form date"),
            43000 => (Self::decode_complex, "complex number"),
            55799 => (Self::decode_self_describe_cbor, "self-described CBOR value"),
            _ => {
                let tag = CBORTag::new(tagnum.into_bound_py_any(py)?, py.None().into_bound(py))?;
                let bound_tag = Bound::new(py, tag)?.into_any();
                let tag_immutable = state.current_immutable;
                return self.push_single_child(
                    state,
                    FrameKind::TagHook {
                        tag: bound_tag.clone().unbind(),
                        tag_immutable,
                    },
                    DisplayName::SemanticTag(tagnum),
                    tag_immutable,
                    Some(bound_tag),
                );
            }
        };
        self.push_single_child(
            state,
            FrameKind::BuiltinTag(transform),
            DisplayName::String(typename),
            true,
            None,
        )
    }

    //
    // Value placement into the innermost frame
    //

    fn place_value<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        value: Bound<'py, PyAny>,
    ) -> PyResult<()> {
        let typename = state.frames.last().unwrap().typename.clone();
        self.place_value_inner(py, state, value)
            .map_err(|e| wrap_decode_error(py, e, &typename))
    }

    fn place_value_inner<'py>(
        &self,
        py: Python<'py>,
        state: &mut AssemblerState,
        value: Bound<'py, PyAny>,
    ) -> PyResult<()> {
        let action = {
            let frame = state.frames.last_mut().unwrap();
            let frame_immutable = frame.immutable;
            match &mut frame.kind {
                FrameKind::Array { storage, remaining } => {
                    match storage {
                        ArrayStorage::List(list) => list.bind(py).append(&value)?,
                        ArrayStorage::Tuple(items) => items.push(value.clone().unbind()),
                    }
                    match remaining {
                        Some(count) => {
                            *count -= 1;
                            if *count == 0 {
                                Action::Complete(Self::build_array(py, storage))
                            } else {
                                Action::Continue(false)
                            }
                        }
                        None => Action::Continue(false),
                    }
                }
                FrameKind::Map {
                    storage,
                    pending_key,
                    remaining,
                    seen_keys,
                    map_immutable,
                } => {
                    if pending_key.is_none() {
                        *pending_key = Some(value.unbind());
                        Action::Continue(false)
                    } else {
                        let key = pending_key.take().unwrap().into_bound(py);
                        if !self.allow_duplicate_keys {
                            let duplicate = match storage {
                                MapStorage::Dict(dict) => dict.bind(py).contains(&key)?,
                                MapStorage::Items(_) => {
                                    let seen = seen_keys.as_ref().unwrap().bind(py);
                                    if seen.contains(&key)? {
                                        true
                                    } else {
                                        seen.add(key.clone())?;
                                        false
                                    }
                                }
                            };
                            if duplicate {
                                return Err(CBORDecodeError::new_err(format!(
                                    "Duplicate map key: {}",
                                    key.repr()?.to_str()?
                                )));
                            }
                        }
                        match storage {
                            MapStorage::Dict(dict) => dict.bind(py).set_item(&key, &value)?,
                            MapStorage::Items(items) => items.push((key.unbind(), value.unbind())),
                        }
                        let map_immutable = *map_immutable;
                        let complete = match remaining {
                            Some(count) => {
                                *count -= 1;
                                *count == 0
                            }
                            None => false,
                        };
                        if complete {
                            Action::Complete(self.build_map(py, storage, map_immutable)?)
                        } else {
                            Action::Continue(true)
                        }
                    }
                }
                FrameKind::Set { set, set_immutable } => {
                    let result = if let Some(set) = set {
                        let bound = set.bind(py);
                        bound.call_method1(intern!(py, "update"), (value,))?;
                        bound.clone().into_any()
                    } else {
                        let _ = set_immutable;
                        let tuple = value.cast_into::<PyTuple>()?;
                        PyFrozenSet::new(py, tuple.iter())?.into_any()
                    };
                    Action::Complete(result)
                }
                FrameKind::BuiltinTag(transform) => Action::Complete(transform(value)?),
                FrameKind::UserSemantic(decoder) => {
                    Action::Complete(decoder.bind(py).call1((value, frame_immutable))?)
                }
                FrameKind::ShareablePhase2(callback) => {
                    Action::Complete(callback.bind(py).call1((value,))?)
                }
                FrameKind::TagHook { tag, tag_immutable } => {
                    let tag_immutable = *tag_immutable;
                    let bound_tag = tag.bind(py).clone();
                    {
                        let cbortag: &Bound<'py, CBORTag> = bound_tag.cast()?;
                        cbortag.borrow_mut().value = value.unbind();
                    }
                    let result = if let Some(tag_hook) = &self.tag_hook {
                        tag_hook.bind(py).call1((&bound_tag, tag_immutable))?
                    } else {
                        bound_tag
                    };
                    Action::Complete(result)
                }
                FrameKind::StringRef => Action::ResolveStringRef(value),
                FrameKind::SharedRef => Action::ResolveSharedRef(value),
                FrameKind::Shareable { index } => Action::CompleteShareable(*index, value),
                FrameKind::StringNamespace => Action::CompletePopNamespace(value),
                FrameKind::ByteStringChunks(_) | FrameKind::TextStringChunks(_) => {
                    unreachable!("string chunk frames are handled in process_token")
                }
            }
        };

        match action {
            Action::Continue(require_immutable) => {
                Self::continue_frame(state, require_immutable);
                Ok(())
            }
            Action::Complete(value) => {
                state.frames.pop();
                Self::after_pop(state);
                state.pending_value = Some(value.unbind());
                Ok(())
            }
            Action::CompleteShareable(index, value) => {
                if state.shareables[index].is_none() {
                    state.shareables[index] = Some(value.clone().unbind());
                }
                state.frames.pop();
                Self::after_pop(state);
                state.pending_value = Some(value.unbind());
                Ok(())
            }
            Action::CompletePopNamespace(value) => {
                state.string_namespaces.pop();
                state.frames.pop();
                Self::after_pop(state);
                state.pending_value = Some(value.unbind());
                Ok(())
            }
            Action::ResolveStringRef(value) => {
                let index: usize = value.extract()?;
                let resolved = match state.string_namespaces.last() {
                    Some(namespace) => match namespace.get(index) {
                        Some(string) => string.bind(py).clone(),
                        None => {
                            return Err(CBORDecodeError::new_err(format!(
                                "string reference {index} not found"
                            )));
                        }
                    },
                    None => {
                        return Err(CBORDecodeError::new_err(
                            "string reference outside of namespace",
                        ));
                    }
                };
                state.frames.pop();
                Self::after_pop(state);
                state.pending_value = Some(resolved.unbind());
                Ok(())
            }
            Action::ResolveSharedRef(value) => {
                let index: usize = value.extract()?;
                let resolved = match state.shareables.get(index) {
                    Some(Some(value)) => value.bind(py).clone(),
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
                state.frames.pop();
                Self::after_pop(state);
                state.pending_value = Some(resolved.unbind());
                Ok(())
            }
        }
    }

    /// Runs the assembler until a complete top-level item is produced, pulling
    /// tokens from the internal stream decoder as needed.
    fn run_decode<'py>(
        &mut self,
        py: Python<'py>,
        state: &mut AssemblerState,
    ) -> PyResult<Bound<'py, PyAny>> {
        loop {
            if let Some(value) = state.pending_value.take() {
                self.place_value(py, state, value.into_bound(py))?;
            } else {
                // The reader is borrowed only to pull the next token; the borrow
                // is released before `process_token` runs. For the inline reader
                // this is a zero-cost compile-time borrow.
                let token = match &mut self.reader {
                    Some(reader) => reader.next_token(py, false)?,
                    None => self
                        .stream
                        .as_ref()
                        .unwrap()
                        .bind(py)
                        .borrow_mut()
                        .next_token(py, false)?,
                }
                .expect("next_token(allow_eof=false) unexpectedly returned None");
                self.process_token(py, state, token)?;
            }

            if state.frames.is_empty() {
                return Ok(state
                    .pending_value
                    .take()
                    .expect("assembler finished without a value")
                    .into_bound(py));
            }
        }
    }

    //
    // Semantic-tag transforms (major type 6)
    //

    fn decode_datetime_string<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 0
        let py = value.py();
        let value_type = value.get_type();
        let mut datetime_str: Bound<'py, PyString> = value.cast_into().map_err(|e| {
            create_exc_from(
                py,
                CBORDecodeError::new_err(format!(
                    "expected string for tag, got {} instead",
                    value_type
                )),
                Some(PyErr::from(e)),
            )
        })?;

        // Python 3.10 has impaired parsing of the ISO format:
        // * It doesn't handle the standard "Z" suffix
        // * It doesn't handle the fractional seconds part having fewer than 6 digits
        if py.version_info() <= (3, 10) {
            let mut temp_str = datetime_str.to_string().replacen("Z", "+00:00", 1);
            if let Some((first, second)) = temp_str.split_once('.')
                && let Some(index) = second.find(|c: char| !c.is_numeric())
            {
                let (mut micros, tz_part) = second.split_at(index);
                if micros.len() >= 6 {
                    micros = &micros[..6];
                }
                temp_str = format!("{first}.{micros:0<6}{tz_part}");
            }
            datetime_str = temp_str.into_pyobject(py)?;
        }

        DATETIME_FROMISOFORMAT.get(py)?.call1((&datetime_str,))
    }

    fn decode_epoch_datetime<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 1
        let py = value.py();
        let utc = UTC.get(py)?;
        DATETIME_FROMTIMESTAMP.get(py)?.call1((value, utc))
    }

    fn decode_positive_bignum<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 2
        let py = value.py();
        INT_FROMBYTES.get(py)?.call1((value, intern!(py, "big")))
    }

    fn decode_negative_bignum<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 3
        let py = value.py();
        let int = INT_FROMBYTES.get(py)?.call1((value, intern!(py, "big")))?;
        int.neg()?.add(-1)
    }

    fn decode_fraction<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 4
        let py = value.py();
        let tuple = require_tuple(value, 2)?;
        let decimal_class = DECIMAL_TYPE.get(py)?;
        let exp = tuple.get_item(0)?;
        let sig_tuple = decimal_class
            .call1((tuple.get_item(1)?,))?
            .call_method0(intern!(py, "as_tuple"))?
            .cast_into::<PyTuple>()?;
        let sign = sig_tuple.get_item(0)?;
        let digits = sig_tuple.get_item(1)?;
        let args_tuple = PyTuple::new(py, [sign, digits, exp])?;
        decimal_class.call1((args_tuple,))
    }

    fn decode_bigfloat<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 5
        let py = value.py();
        let tuple = require_tuple(value, 2)?;
        let decimal_class = DECIMAL_TYPE.get(py)?;
        let exp = decimal_class.call1((tuple.get_item(0)?,))?;
        let sig = decimal_class.call1((tuple.get_item(1)?,))?;
        let exp = PyInt::new(py, 2).pow(exp, py.None())?;
        sig.mul(exp)
    }

    fn decode_rational<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 30
        let py = value.py();
        let tuple = require_tuple(value, 2)?;
        FRACTION_TYPE.get(py)?.call1(tuple)
    }

    fn decode_regexp<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 35
        RE_COMPILE.get(value.py())?.call1((value,))
    }

    fn decode_mime<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 36
        let py = value.py();
        let parser = EMAIL_PARSER.get(py)?.call0()?;
        parser.call_method1(intern!(py, "parsestr"), (value,))
    }

    fn decode_uuid<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 37
        let py = value.py();
        let kwargs = PyDict::new(py);
        kwargs.set_item(intern!(py, "bytes"), value)?;
        UUID_TYPE.get(py)?.call((), Some(&kwargs))
    }

    fn decode_ipv4<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 52
        let py = value.py();
        let addr = if let Ok(bytes) = value.cast::<PyBytes>() {
            IPV4ADDRESS_TYPE.get(py)?.call1((bytes,))?
        } else if let Ok(tuple) = value.cast_into::<PyTuple>()
            && tuple.len() == 2
        {
            let first_item = tuple.get_item(0)?;
            let second_item = tuple.get_item(1)?;
            if let Ok(prefix) = first_item.cast::<PyInt>()
                && let Ok(address) = second_item.cast::<PyBytes>()
            {
                let mut address_vec: Vec<u8> = address.extract()?;
                if address_vec.len() > 4 {
                    return Err(CBORDecodeError::new_err(format!(
                        "address byte string for IPv4 network is too long ({} bytes)",
                        address_vec.len()
                    )));
                }
                address_vec.resize(4, 0);
                IPV4NETWORK_TYPE.get(py)?.call1(((address_vec, prefix),))?
            } else if let Ok(address) = first_item.cast::<PyBytes>()
                && let Ok(prefix) = second_item.cast::<PyInt>()
            {
                IPV4INTERFACE_TYPE.get(py)?.call1(((address, prefix),))?
            } else {
                return Err(CBORDecodeError::new_err("invalid types in input array"));
            }
        } else {
            return Err(CBORDecodeError::new_err(
                "input value must be a bytestring or an array of 2 elements",
            ));
        };
        Ok(addr)
    }

    fn decode_ipv6<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 54
        let py = value.py();
        let ipv6addr_class = IPV6ADDRESS_TYPE.get(py)?;
        let addr = if let Ok(bytes) = value.cast::<PyBytes>() {
            ipv6addr_class.call1((bytes,))?
        } else if let Ok(tuple) = value.cast_into::<PyTuple>()
            && (2..=3).contains(&tuple.len())
        {
            let first_item = tuple.get_item(0)?;
            let second_item = tuple.get_item(1)?;
            let zone_id = tuple.get_item(2).ok();

            let zone_id_suffix = if let Some(zone_id) = zone_id {
                if let Ok(zone_id_bytes) = zone_id.cast::<PyBytes>() {
                    let zone_id_str = String::from_utf8(zone_id_bytes.as_bytes().to_vec())?;
                    format!("%{zone_id_str}")
                } else if let Ok(zone_id_int) = zone_id.cast::<PyInt>() {
                    format!("%{zone_id_int}")
                } else {
                    return Err(CBORDecodeError::new_err(
                        "zone ID must be an integer or a bytestring",
                    ));
                }
            } else {
                String::default()
            };

            if second_item.is_none()
                && let Ok(address) = first_item.cast::<PyBytes>()
            {
                let addr_obj = ipv6addr_class.call1((address,))?;
                ipv6addr_class.call1((format!("{addr_obj}{zone_id_suffix}"),))?
            } else {
                let (class, addr_bytes, prefix) = if let Ok(prefix) = first_item.cast::<PyInt>()
                    && let Ok(address) = second_item.cast::<PyBytes>()
                {
                    let mut address_vec: Vec<u8> = address.extract()?;
                    if address_vec.len() > 16 {
                        return Err(CBORDecodeError::new_err(format!(
                            "address byte string for IPv6 network is too long ({} bytes)",
                            address_vec.len()
                        )));
                    }
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
                    Err(CBORDecodeError::new_err("invalid types in input array"))
                }?;
                let addr_obj = ipv6addr_class.call1((addr_bytes,))?;
                let formatted_addr = format!("{addr_obj}{zone_id_suffix}/{prefix}");
                class.call1((formatted_addr,))?
            }
        } else {
            return Err(CBORDecodeError::new_err(
                "input value must be a bytestring or an array of 2 elements",
            ));
        };
        Ok(addr)
    }

    fn decode_epoch_date<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 100
        let py = value.py();
        let value = value.extract::<i32>()? + 719163;
        DATE_FROMORDINAL.get(py)?.call1((value,))
    }

    fn decode_ipaddress<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 260 (deprecated)
        let py = value.py();
        let value = value.cast_into::<PyBytes>()?;
        match value.len()? {
            4 | 16 => IPADDRESS_FUNC.get(py)?.call1((value,)),
            6 => Ok(Bound::new(py, CBORTag::new_internal(260, value.into_any()))?.into_any()),
            length => Err(CBORDecodeError::new_err(format!(
                "invalid IP address length ({length})"
            ))),
        }
    }

    fn decode_ipnetwork<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 261 (deprecated)
        let py = value.py();
        let value: Bound<'py, PyMapping> = value.cast_into()?;
        let length = value.len()?;
        if length != 1 {
            return Err(CBORDecodeError::new_err(format!(
                "invalid input map length for IP network: {}",
                length
            )));
        }
        let first_item = value.items()?.get_item(0)?;
        let mask_length = first_item.get_item(1)?;
        if !mask_length.is_exact_instance_of::<PyInt>() {
            return Err(CBORDecodeError::new_err(format!(
                "invalid mask length for IP network: {mask_length}"
            )));
        }

        match IPNETWORK_FUNC.get(py)?.call1((&first_item,)) {
            Ok(ip_network) => Ok(ip_network),
            Err(e) => {
                if e.is_instance_of::<PyValueError>(py) {
                    IPINTERFACE_FUNC.get(py)?.call1((first_item,))
                } else {
                    Err(e)
                }
            }
        }
    }

    fn decode_date_string<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 1004
        let py = value.py();
        DATE_FROMISOFORMAT.get(py)?.call1((value,))
    }

    fn decode_complex<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 43000
        let py = value.py();
        let tuple = require_tuple(value, 2)?;
        let real: f64 = tuple.get_item(0)?.extract()?;
        let imag: f64 = tuple.get_item(1)?.extract()?;
        Ok(PyComplex::from_doubles(py, real, imag).into_any())
    }

    fn decode_self_describe_cbor<'py>(value: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        // Semantic tag 55799
        Ok(value)
    }
}

#[cfg(Py_3_15)]
fn create_frozen_dict<'py>(
    py: Python<'py>,
    items: Vec<(Bound<'py, PyAny>, Bound<'py, PyAny>)>,
) -> PyResult<Bound<'py, PyAny>> {
    FROZEN_DICT
        .get(py)?
        .call1((items,))?
        .cast_into()
        .map_err(PyErr::from)
}

#[cfg(not(Py_3_15))]
fn create_frozen_dict<'py>(
    py: Python<'py>,
    items: Vec<(Bound<'py, PyAny>, Bound<'py, PyAny>)>,
) -> PyResult<Bound<'py, PyAny>> {
    FrozenDict::from_items(py, items).map(|dict| dict.into_any())
}

fn token_major_type(token: &Token<'_>) -> u8 {
    match token {
        Token::Integer(_) => 0,
        Token::ByteString(..) | Token::ByteStringStart => 2,
        Token::TextString(..) | Token::TextStringStart => 3,
        Token::ArrayStart(_) => 4,
        Token::MapStart(_) => 5,
        Token::Tag(_) => 6,
        Token::Simple(_)
        | Token::Float(_)
        | Token::Boolean(_)
        | Token::Null
        | Token::Undefined
        | Token::Break => 7,
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
        token_hooks = None,
        str_errors = "strict",
        read_size = 4096,
        max_depth = 400,
        allow_indefinite = true,
        allow_duplicate_keys = true,
    ))]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        py: Python<'_>,
        fp: &Bound<'_, PyAny>,
        tag_hook: Option<&Bound<'_, PyAny>>,
        object_hook: Option<&Bound<'_, PyAny>>,
        semantic_decoders: Option<&Bound<'_, PyMapping>>,
        token_hooks: Option<&Bound<'_, PyMapping>>,
        str_errors: &str,
        read_size: usize,
        max_depth: usize,
        allow_indefinite: bool,
        allow_duplicate_keys: bool,
    ) -> PyResult<Self> {
        Self::new_internal(
            py,
            Some(fp),
            None,
            tag_hook,
            object_hook,
            semantic_decoders,
            token_hooks,
            str_errors,
            read_size,
            max_depth,
            allow_indefinite,
            allow_duplicate_keys,
        )
    }

    /// The underlying low-level :class:`~cbor2.CBORStreamDecoder`.
    ///
    /// Accessing this promotes the inline stream decoder to a shared Python
    /// object (the first access allocates it); subsequent decoding goes through
    /// that shared object.
    #[getter]
    fn stream(&mut self, py: Python<'_>) -> PyResult<Py<CBORStreamDecoder>> {
        if let Some(reader) = self.reader.take() {
            let stream = Bound::new(py, reader)?.unbind();
            self.stream = Some(stream.clone_ref(py));
            Ok(stream)
        } else {
            Ok(self.stream.as_ref().unwrap().clone_ref(py))
        }
    }

    #[getter]
    fn fp(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.with_reader(py, |reader| reader.fp(py))
    }

    #[setter]
    fn set_fp(&mut self, fp: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = fp.py();
        self.with_reader_mut(py, |reader| reader.set_fp(fp))
    }

    #[getter]
    fn read_size(&self, py: Python<'_>) -> usize {
        self.with_reader(py, |reader| reader.read_size)
    }

    #[getter]
    fn str_errors(&self, py: Python<'_>) -> Py<PyString> {
        self.with_reader(py, |reader| reader.str_errors(py))
    }

    #[setter]
    fn set_str_errors(&mut self, str_errors: &Bound<'_, PyString>) -> PyResult<()> {
        let py = str_errors.py();
        self.with_reader_mut(py, |reader| reader.set_str_errors(str_errors))
    }

    #[getter]
    fn tag_hook(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.tag_hook.as_ref().map(|hook| hook.clone_ref(py))
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
        self.object_hook.as_ref().map(|hook| hook.clone_ref(py))
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
    fn token_hooks(&self, py: Python<'_>) -> Option<Py<PyMapping>> {
        self.token_hooks_map.as_ref().map(|m| m.clone_ref(py))
    }

    #[setter]
    fn set_token_hooks(&mut self, token_hooks: Option<&Bound<'_, PyMapping>>) -> PyResult<()> {
        let mut hooks: [Option<Py<PyAny>>; NUM_TOKEN_HOOKS] = std::array::from_fn(|_| None);
        let mut any = false;
        if let Some(mapping) = token_hooks {
            let items = mapping.items()?;
            for item in items.try_iter()? {
                let item = item?;
                let token_type = item.get_item(0)?;
                let callback = item.get_item(1)?;
                if !callback.is_callable() {
                    return Err(PyTypeError::new_err("token_hooks values must be callable"));
                }
                let index = token_hook_kind_index(&token_type)?;
                hooks[index] = Some(callback.unbind());
                any = true;
            }
            self.token_hooks_map = Some(mapping.clone().unbind());
        } else {
            self.token_hooks_map = None;
        }
        self.token_hooks = hooks;
        self.any_token_hooks = any;
        Ok(())
    }

    /// Read bytes from the data stream.
    ///
    /// :param amount: the number of bytes to read
    #[pyo3(signature = (amount, /))]
    fn read(&mut self, py: Python<'_>, amount: usize) -> PyResult<Vec<u8>> {
        self.with_reader_mut(py, |reader| reader.read(py, amount))
    }

    /// Decode the next value from the stream.
    ///
    /// :param immutable: if :data:`True`, decode the next item as an immutable type
    ///     (e.g. :class:`tuple` instead of a :class:`list`), if possible
    /// :return: the decoded object
    /// :raises CBORDecodeError: if there is any problem decoding the stream
    #[pyo3(signature = (*, immutable = false))]
    pub fn decode<'py>(&mut self, py: Python<'py>, immutable: bool) -> PyResult<Bound<'py, PyAny>> {
        let mut state = AssemblerState::new(immutable);
        let value = self.run_decode(py, &mut state)?;
        self.with_reader_mut(py, |reader| reader.rewind_buffer(py))?;
        Ok(value)
    }
}
