#pragma once
#include "bridge.h"
#include <firebird/Interface.h>
#include <cstring>
#include <map>
#include <memory>
#include <mutex>
#include <stdexcept>

using namespace Firebird;

struct Connection;
struct ControlState {
    // Only the independent cancel call holds this across network I/O. Normal
    // query calls do not take it. End/invalidate must wait before cleanup/detach.
    std::mutex gate;
    Connection* owner = nullptr;
    uint64_t generation = 0;
    bool raised = false;
    bool invalidate() {
        std::lock_guard<std::mutex> lock(gate);
        owner = nullptr; generation = 0;
        const bool was_raised = raised; raised = false; return was_raised;
    }
};

struct Prepared {
    IStatement* handle = nullptr;
    IMessageMetadata* input = nullptr;
    IMessageMetadata* output = nullptr;
    ~Prepared() {
        if (input) input->release();
        if (output) output->release();
        if (handle) handle->release();
    }
};

struct Connection {
    IMaster* master;
    SqlStateFn sqlstate;
    IAttachment* attachment = nullptr;
    ITransaction* transaction = nullptr;
    IResultSet* cursor = nullptr;
    IBlob* blob = nullptr;
    uint64_t active_statement = 0;
    uint64_t next_statement = 1;
    std::map<uint64_t, std::unique_ptr<Prepared>> statements;
    std::shared_ptr<ControlState> control;
    Connection(IMaster* m, SqlStateFn s) : master(m), sqlstate(s), control(std::make_shared<ControlState>()) { control->owner = this; }
    ~Connection() {
        control->invalidate();
        // Reference release is the last-resort local handle cleanup. Protocol
        // close/rollback/detach is explicit on the owning worker before this.
        if (blob) blob->release();
        if (cursor) cursor->release();
        statements.clear();
        if (transaction) transaction->release();
        if (attachment) attachment->release();
    }
    Prepared& statement(uint64_t id) {
        auto found = statements.find(id);
        if (found == statements.end()) throw std::logic_error("Unknown or expired statement");
        return *found->second;
    }
};

inline void require(bool valid, const char* message) {
    if (!valid) throw std::logic_error(message);
}

void capture(Connection*, IStatus*, FbError*) noexcept;

template<class Function>
void guarded(Connection* connection, FbError* error, Function operation) noexcept {
    std::memset(error, 0, sizeof(*error));
    IStatus* raw = nullptr;
    try {
        raw = connection->master->getStatus();
        ThrowStatusWrapper status(raw);
        operation(&status);
    } catch (const FbException& e) {
        capture(connection, e.getStatus(), error);
    } catch (const std::bad_alloc&) {
        error->category = 3;
        std::strcpy(error->message, "Native allocation failed");
    } catch (const std::logic_error& e) {
        error->category = 2;
        std::strncpy(error->message, e.what(), sizeof(error->message) - 1);
    } catch (...) {
        error->category = 2;
        std::strcpy(error->message, "Unexpected exception at Firebird ABI boundary");
    }
    if (raw) raw->dispose();
}
