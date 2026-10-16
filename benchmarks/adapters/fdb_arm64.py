"""Explicit benchmark-only workaround for FDB 2.0.4's Apple arm64 variadic ABI.

FDB declares all six arguments fixed although ibase.h declares only three plus
ellipsis. Apple arm64 passes the variadic tail differently. This changes neither
decode nor fetch nor transaction policy. It does not modify the installed driver.
https://docs.python.org/3/library/ctypes.html#calling-variadic-functions
"""
import ctypes
import platform


def configure(fdb, library):
    if (platform.system(), platform.machine()) != ("Darwin", "arm64"):
        raise RuntimeError("The fdb-arm64 adapter is specific to Apple arm64")
    fdb.load_api(library)
    api = fdb.fbcore.api
    start = api.isc_start_transaction
    start.argtypes = start.argtypes[:3]

    def start_one(status, transaction, count, database, length, tpb):
        if count != 1:
            raise RuntimeError("The benchmark owns exactly one attachment")
        return start(status, transaction, count, ctypes.byref(database),
                     ctypes.c_int(length), ctypes.c_char_p(tpb))

    api.isc_start_transaction = start_one
