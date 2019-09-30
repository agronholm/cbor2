#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include <stdbool.h>
#include <stdint.h>

typedef struct {
    PyObject_HEAD
    PyObject *read;    // cached read() method of fp
    PyObject *tag_hook;
    PyObject *object_hook;
    PyObject *shareables;
    PyObject *str_errors;
    bool immutable;
    Py_ssize_t shared_index;
} CBORDecoderObject;

PyTypeObject CBORDecoderType;

int fp_read(CBORDecoderObject *, char *, const uint64_t);
PyObject * CBORDecoder_new(PyTypeObject *, PyObject *, PyObject *);
int CBORDecoder_init(CBORDecoderObject *, PyObject *, PyObject *);
PyObject * CBORDecoder_decode(CBORDecoderObject *);
PyObject * decode_with_lead_byte(CBORDecoderObject *, LeadByte);
