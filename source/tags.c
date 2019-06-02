#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include <stdint.h>
#include "structmember.h"
#include "tags.h"


// Constructors and destructors //////////////////////////////////////////////

static int
CBORTag_traverse(CBORTagObject *self, visitproc visit, void *arg)
{
    Py_VISIT(self->value);
    return 0;
}

static int
CBORTag_clear(CBORTagObject *self)
{
    Py_CLEAR(self->value);
    return 0;
}

// CBORTag.__del__(self)
static void
CBORTag_dealloc(CBORTagObject *self)
{
    PyObject_GC_UnTrack(self);
    CBORTag_clear(self);
    Py_TYPE(self)->tp_free((PyObject *) self);
}


// CBORTag.__new__(cls, *args, **kwargs)
static PyObject *
CBORTag_new(PyTypeObject *type, PyObject *args, PyObject *kwargs)
{
    CBORTagObject *self;

    self = (CBORTagObject *) type->tp_alloc(type, 0);
    if (self) {
        self->tag = 0;
        Py_INCREF(Py_None);
        self->value = Py_None;
    }
    return (PyObject *) self;
}


// CBORTag.__init__(self, tag=None, value=None)
static int
CBORTag_init(CBORTagObject *self, PyObject *args, PyObject *kwargs)
{
    static char *keywords[] = {"tag", "value", NULL};
    PyObject *tmp, *value = NULL;

    if (!PyArg_ParseTupleAndKeywords(args, kwargs, "|KO", keywords,
                &self->tag, &value))
        return -1;

    if (value) {
        tmp = self->value;
        Py_INCREF(value);
        self->value = value;
        Py_XDECREF(tmp);
    }
    return 0;
}


// Special methods ///////////////////////////////////////////////////////////

static PyObject *
CBORTag_repr(CBORTagObject *self)
{
    PyObject *ret = NULL;

    if (Py_ReprEnter((PyObject *)self))
        ret = PyUnicode_FromString("...");
    else
        ret = PyUnicode_FromFormat("CBORTag(%llu, %R)", self->tag, self->value);
    Py_ReprLeave((PyObject *)self);
    return ret;
}


static PyObject *
CBORTag_richcompare(PyObject *aobj, PyObject *bobj, int op)
{
    PyObject *ret = NULL;
    CBORTagObject *a, *b;

    if (!(CBORTag_CheckExact(aobj) && CBORTag_CheckExact(bobj))) {
        Py_RETURN_NOTIMPLEMENTED;
    } else {
        a = (CBORTagObject *)aobj;
        b = (CBORTagObject *)bobj;

        if (a == b) {
            // Special case: both are the same object
            switch (op) {
                case Py_EQ: case Py_LE: case Py_GE: ret = Py_True; break;
                case Py_NE: case Py_LT: case Py_GT: ret = Py_False; break;
                default: assert(0);
            }
            Py_INCREF(ret);
        } else if (a->tag == b->tag) {
            // Tags are equal, rich-compare the value
            ret = PyObject_RichCompare(a->value, b->value, op);
        } else {
            // Tags differ; simple integer comparison
            switch (op) {
                case Py_EQ: ret = Py_False; break;
                case Py_NE: ret = Py_True;  break;
                case Py_LT: ret = a->tag <  b->tag ? Py_True : Py_False; break;
                case Py_LE: ret = a->tag <= b->tag ? Py_True : Py_False; break;
                case Py_GE: ret = a->tag >= b->tag ? Py_True : Py_False; break;
                case Py_GT: ret = a->tag >  b->tag ? Py_True : Py_False; break;
                default: assert(0);
            }
            Py_INCREF(ret);
        }
    }
    return ret;
}


// C API /////////////////////////////////////////////////////////////////////

PyObject *
CBORTag_New(uint64_t tag)
{
    CBORTagObject *ret = NULL;

    ret = PyObject_GC_New(CBORTagObject, &CBORTagType);
    if (ret) {
        ret->tag = tag;
        Py_INCREF(Py_None);
        ret->value = Py_None;
    }
    return (PyObject *)ret;
}

int
CBORTag_SetValue(PyObject *tag, PyObject *value)
{
    PyObject *tmp;
    CBORTagObject *self;

    if (!CBORTag_CheckExact(tag))
        return -1;
    if (!value)
        return -1;

    self = (CBORTagObject*)tag;
    tmp = self->value;
    Py_INCREF(value);
    self->value = value;
    Py_XDECREF(tmp);
    return 0;
}


// Tag class definition //////////////////////////////////////////////////////

static PyMemberDef CBORTag_members[] = {
    {"tag", T_ULONGLONG, offsetof(CBORTagObject, tag), 0,
        "the semantic tag associated with the value"},
    {"value", T_OBJECT_EX, offsetof(CBORTagObject, value), 0,
        "the tagged value"},
    {NULL}
};

PyDoc_STRVAR(CBORTag__doc__,
"The CBORTag class represents a semantically tagged value in a CBOR\n"
"encoded stream. The :attr:`tag` attribute holds the numeric tag\n"
"associated with the stored :attr:`value`.\n"
);

PyTypeObject CBORTagType = {
    PyVarObject_HEAD_INIT(NULL, 0)
    .tp_name = "_cbor2.CBORTag",
    .tp_doc = CBORTag__doc__,
    .tp_basicsize = sizeof(CBORTagObject),
    .tp_itemsize = 0,
    .tp_flags = Py_TPFLAGS_DEFAULT | Py_TPFLAGS_HAVE_GC,
    .tp_new = CBORTag_new,
    .tp_init = (initproc) CBORTag_init,
    .tp_dealloc = (destructor) CBORTag_dealloc,
    .tp_traverse = (traverseproc) CBORTag_traverse,
    .tp_clear = (inquiry) CBORTag_clear,
    .tp_members = CBORTag_members,
    .tp_repr = (reprfunc) CBORTag_repr,
    .tp_richcompare = CBORTag_richcompare,
};
