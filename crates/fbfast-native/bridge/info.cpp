#include "state.h"

// Integer attachment information items (wire statistics, protocol version and
// inline BLOB limits). Values are unsigned little-endian per the Firebird info
// format. Absent, unknown (isc_info_error) or oversized items report UINT64_MAX.
extern "C" void fbfast_attachment_info(void* handle, const uint8_t* items,
    uint32_t count, uint64_t* values, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(c->attachment && items && values && count > 0 && count <= 32,
            "Invalid attachment info request");
        for (uint32_t i = 0; i < count; ++i) values[i] = UINT64_MAX;
        unsigned char buffer[512];
        c->attachment->getInfo(status, count, items, sizeof(buffer), buffer);
        const unsigned char* p = buffer;
        const unsigned char* const end = buffer + sizeof(buffer);
        while (p < end && *p != isc_info_end) {
            const unsigned char tag = *p++;
            require(tag != isc_info_truncated, "Attachment info truncated");
            require(end - p >= 2, "Malformed attachment info");
            const unsigned length = static_cast<unsigned>(p[0]) | (static_cast<unsigned>(p[1]) << 8);
            p += 2;
            require(static_cast<size_t>(end - p) >= length, "Malformed attachment info");
            if (tag != isc_info_error && length > 0 && length <= 8) {
                uint64_t value = 0;
                for (unsigned b = 0; b < length; ++b) value |= static_cast<uint64_t>(p[b]) << (8 * b);
                for (uint32_t i = 0; i < count; ++i) if (items[i] == tag) values[i] = value;
            }
            p += length;
        }
    });
}
