#include "state.h"

static IMessageMetadata* metadata(Connection* c, uint64_t id, uint32_t input) {
    auto& s = c->statement(id);
    return input ? s.input : s.output;
}

extern "C" uint32_t fbfast_message_length(void* handle, uint64_t id, uint32_t input, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    uint32_t size = 0;
    guarded(c, error, [&](auto* status) { size = metadata(c, id, input)->getMessageLength(status); });
    return size;
}

extern "C" uint32_t fbfast_column_count(void* handle, uint64_t id, uint32_t input, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    uint32_t count = 0;
    guarded(c, error, [&](auto* status) { count = metadata(c, id, input)->getCount(status); });
    return count;
}

extern "C" void fbfast_column(void* handle, uint64_t id, uint32_t input,
    uint32_t index, FbColumn* out, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        auto* m = metadata(c, id, input);
        require(index < m->getCount(status), "Column index out of range");
        std::memset(out, 0, sizeof(*out));
        out->type = m->getType(status, index) & ~1u;
        out->length = m->getLength(status, index);
        out->charset = m->getCharSet(status, index);
        out->offset = m->getOffset(status, index);
        out->null_offset = m->getNullOffset(status, index);
        out->scale = m->getScale(status, index);
        out->subtype = m->getSubType(status, index);
        const char* name = m->getAlias(status, index);
        if (name) {
            require(std::strlen(name) < sizeof(out->name), "Column alias exceeds bridge capacity");
            std::strcpy(out->name, name);
        }
    });
}
