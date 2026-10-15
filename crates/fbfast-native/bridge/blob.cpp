#include "state.h"

extern "C" void fbfast_open_blob(void* handle, const uint8_t* id,
    uint32_t charset, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(c->transaction && c->cursor && !c->blob, "BLOB requires an exclusive active cursor");
        require(charset <= 65535, "Invalid BLOB charset");
        ISC_QUAD blob_id;
        static_assert(sizeof(blob_id) == 8);
        std::memcpy(&blob_id, id, sizeof(blob_id));
        // Preserve NONE/OCTETS/binary bytes. For known text encodings ask the
        // official client/server to transliterate to UTF8; never guess locally.
        const unsigned char bpb[] = {isc_bpb_version1,
            isc_bpb_source_type, 2, 1, 0, isc_bpb_target_type, 2, 1, 0,
            isc_bpb_source_interp, 2, static_cast<unsigned char>(charset),
            static_cast<unsigned char>(charset >> 8), isc_bpb_target_interp, 2, 4, 0};
        c->blob = c->attachment->openBlob(status, c->transaction, &blob_id,
            charset > 1 ? sizeof(bpb) : 0, charset > 1 ? bpb : nullptr);
    });
}

extern "C" uint32_t fbfast_read_blob(void* handle, void* buffer,
    uint32_t capacity, uint32_t* eof, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    unsigned count = 0;
    guarded(c, error, [&](auto* status) {
        require(c->blob && capacity > 0 && capacity <= 65535, "Invalid BLOB read buffer");
        *eof = 0;
        // Stored segments may be only one byte long (e.g. RPAD-created BLOBs).
        // Coalesce them without one C ABI crossing per tiny segment, while
        // bounding calls even for empty segments so Rust can check its deadline.
        for (unsigned segments = 0; count < capacity && segments < 256; ++segments) {
            unsigned actual = 0;
            const int result = c->blob->getSegment(status, capacity - count,
                static_cast<char*>(buffer) + count, &actual);
            require(result == IStatus::RESULT_OK || result == IStatus::RESULT_SEGMENT ||
                result == IStatus::RESULT_NO_DATA, "Unexpected BLOB segment status");
            require(actual <= capacity - count, "BLOB segment exceeds buffer");
            count += actual;
            if (result == IStatus::RESULT_NO_DATA) { *eof = 1; break; }
        }
    });
    return count;
}

extern "C" void fbfast_close_blob(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        if (c->blob) { c->blob->close(status); c->blob = nullptr; }
    });
}
