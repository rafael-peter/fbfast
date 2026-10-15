#include "state.h"

extern "C" void fbfast_decimal_text(void* handle, uint32_t type,
    const uint8_t* data, char* out, uint32_t capacity, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        auto* util = c->master->getUtilInterface();
        if (type == SQL_DEC16) {
            FB_DEC16 value;
            std::memcpy(&value, data, sizeof(value));
            util->getDecFloat16(status)->toString(status, &value, capacity, out);
        } else {
            require(type == SQL_DEC34, "Unsupported decimal storage type");
            FB_DEC34 value;
            std::memcpy(&value, data, sizeof(value));
            util->getDecFloat34(status)->toString(status, &value, capacity, out);
        }
    });
}

extern "C" void fbfast_decimal_bytes(void* handle, uint32_t type,
    const char* text, uint8_t* out, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        auto* util = c->master->getUtilInterface();
        if (type == SQL_DEC16) {
            FB_DEC16 value;
            util->getDecFloat16(status)->fromString(status, text, &value);
            std::memcpy(out, &value, sizeof(value));
        } else {
            require(type == SQL_DEC34, "Unsupported decimal storage type");
            FB_DEC34 value;
            util->getDecFloat34(status)->fromString(status, text, &value);
            std::memcpy(out, &value, sizeof(value));
        }
    });
}
