:mod:`cbor2.tool`
==================

.. automodule:: cbor2.tool


Command line Usage::

    usage: python -m cbor2.tool [-h] [-o OUTFILE] [-k] [-p] [-s] [-d]
                                [-i TAG_IGNORE]
                                [infiles [infiles ...]]

    A simple command line interface for cbor2 module to validate and pretty-print
    CBOR objects.

    positional arguments:
      infiles               Collection of CBOR files to process or - for stdin

    optional arguments:
      -h, --help            show this help message and exit
      -o OUTFILE, --outfile OUTFILE
                            output file
      -k, --sort-keys       sort the output of dictionaries alphabetically by key
      -p, --pretty          indent the output to look good
      -s, --sequence        Parse a sequence of concatenated CBOR items
      -d, --decode          CBOR data is base64 encoded (handy for stdin)
      -i TAG_IGNORE, --tag-ignore TAG_IGNORE
                            Comma separated list of tags to ignore and only return
                            the value

