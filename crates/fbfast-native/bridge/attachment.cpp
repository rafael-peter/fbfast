#include "state.h"

extern "C" void* fbfast_attach(void* master, SqlStateFn sqlstate,
    const FbAttach* options, FbError* error) noexcept {
    Connection* raw = nullptr;
    try { raw = new Connection(static_cast<IMaster*>(master), sqlstate); }
    catch (...) { std::memset(error, 0, sizeof(*error)); error->category = 3; return nullptr; }
    std::unique_ptr<Connection> connection(raw);
    guarded(raw, error, [&](auto* status) {
        auto* dpb = raw->master->getUtilInterface()->getXpbBuilder(status, IXpbBuilder::DPB, nullptr, 0);
        IProvider* provider = nullptr;
        try {
            dpb->insertString(status, isc_dpb_user_name, options->user);
            dpb->insertString(status, isc_dpb_password, options->password);
            if (*options->role) dpb->insertString(status, isc_dpb_sql_role_name, options->role);
            dpb->insertString(status, isc_dpb_lc_ctype, options->charset);
            dpb->insertString(status, isc_dpb_session_time_zone, options->time_zone);
            dpb->insertString(status, isc_dpb_process_name, options->app_name);
            dpb->insertString(status, isc_dpb_config, options->config);
            dpb->insertInt(status, isc_dpb_connect_timeout, options->connect_seconds);
            provider = raw->master->getDispatcher();
            raw->attachment = provider->attachDatabase(status, options->database,
                dpb->getBufferLength(status), dpb->getBuffer(status));
            provider->release(); provider = nullptr;
            dpb->dispose();
        } catch (...) {
            if (provider) provider->release();
            dpb->dispose();
            throw;
        }
    });
    return error->category ? nullptr : connection.release();
}

extern "C" void fbfast_disconnect(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(!c->cursor && !c->transaction && c->statements.empty(), "Detach requires completed cleanup");
        if (c->attachment) { c->attachment->detach(status); c->attachment = nullptr; }
    });
}

extern "C" void fbfast_release(void* handle) noexcept { delete static_cast<Connection*>(handle); }

extern "C" void fbfast_ping(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) { c->attachment->ping(status); });
}

extern "C" void fbfast_reset(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(!c->transaction && !c->cursor, "Reset requires no active transaction or cursor");
        c->transaction = c->attachment->execute(status, nullptr, 0, "ALTER SESSION RESET", 3,
            nullptr, nullptr, nullptr, nullptr);
        require(!c->transaction, "Reset unexpectedly opened a transaction");
    });
}
