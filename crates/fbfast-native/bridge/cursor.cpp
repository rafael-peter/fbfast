#include "state.h"

extern "C" void fbfast_open_cursor(void* handle, uint64_t id, const void* data,
    uint32_t length, uint32_t timeout_ms, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(c->transaction && !c->cursor, "Execute requires an exclusive transaction");
        auto& s = c->statement(id);
        require(length == s.input->getMessageLength(status), "Invalid input message length");
        s.handle->setTimeout(status, timeout_ms);
        c->cursor = s.handle->openCursor(status, c->transaction,
            length ? s.input : nullptr, const_cast<void*>(data), s.output, 0);
        c->active_statement = id;
    });
}

extern "C" uint32_t fbfast_fetch(void* handle, void* data, uint32_t stride,
    uint32_t capacity, uint32_t* eof, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    uint32_t rows = 0;
    guarded(c, error, [&](auto* status) {
        require(c->cursor && c->active_statement, "No active cursor");
        auto& s = c->statement(c->active_statement);
        require(stride >= s.output->getMessageLength(status), "Output message buffer too small");
        *eof = 0;
        for (; rows < capacity; ++rows) {
            int code = c->cursor->fetchNext(status, static_cast<char*>(data) + rows * size_t(stride));
            if (code == IStatus::RESULT_NO_DATA) { *eof = 1; break; }
            require(code == IStatus::RESULT_OK, "Unexpected fetch status");
        }
    });
    return rows;
}

extern "C" void fbfast_close_cursor(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        if (c->cursor) { c->cursor->close(status); c->cursor = nullptr; }
        c->active_statement = 0;
    });
}
