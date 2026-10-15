#include "state.h"
#include <limits>

extern "C" uint64_t fbfast_prepare(void* handle, const char* sql, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    uint64_t id = 0;
    guarded(c, error, [&](auto* status) {
        require(c->transaction && !c->cursor, "Prepare requires an exclusive transaction");
        require(c->next_statement < std::numeric_limits<uint64_t>::max(), "Statement id exhausted");
        auto statement = std::make_unique<Prepared>();
        statement->handle = c->attachment->prepare(status, c->transaction, 0, sql, 3,
            IStatement::PREPARE_PREFETCH_METADATA);
        if (statement->handle->getType(status) != isc_info_sql_stmt_select) {
            statement->handle->free(status); statement->handle = nullptr;
            throw std::logic_error("Prepared statement is not a read-only SELECT");
        }
        statement->input = statement->handle->getInputMetadata(status);
        statement->output = statement->handle->getOutputMetadata(status);
        id = c->next_statement++;
        c->statements.emplace(id, std::move(statement));
    });
    return id;
}

extern "C" void fbfast_free_statement(void* handle, uint64_t id, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(c->active_statement != id, "Statement is pinned by an open cursor");
        auto& s = c->statement(id);
        s.handle->free(status); s.handle = nullptr;
        c->statements.erase(id);
    });
}

extern "C" void fbfast_plan(void* handle, uint64_t id, char* out, uint32_t capacity,
    uint32_t* required, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        const char* plan = c->statement(id).handle->getPlan(status, true);
        *required = plan ? static_cast<uint32_t>(std::strlen(plan) + 1) : 1;
        if (capacity >= *required) {
            if (plan) std::memcpy(out, plan, *required);
            else out[0] = 0;
        }
    });
}
