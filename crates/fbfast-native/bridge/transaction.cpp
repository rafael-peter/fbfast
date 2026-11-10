#include "state.h"

extern "C" void fbfast_begin(void* handle, uint32_t snapshot, uint32_t lock_seconds,
    uint64_t snapshot_number, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(!c->transaction, "Transaction already active");
        require(snapshot || !snapshot_number, "Shared snapshot requires SNAPSHOT isolation");
        auto* tpb = c->master->getUtilInterface()->getXpbBuilder(status, IXpbBuilder::TPB, nullptr, 0);
        try {
            tpb->insertTag(status, isc_tpb_read);
            if (snapshot) tpb->insertTag(status, isc_tpb_concurrency);
            else {
                tpb->insertTag(status, isc_tpb_read_committed);
                tpb->insertTag(status, isc_tpb_read_consistency);
            }
            tpb->insertTag(status, lock_seconds ? isc_tpb_wait : isc_tpb_nowait);
            if (lock_seconds) tpb->insertInt(status, isc_tpb_lock_timeout, lock_seconds);
            if (snapshot_number) tpb->insertBigInt(status, isc_tpb_at_snapshot_number, snapshot_number);
            c->transaction = c->attachment->startTransaction(status,
                tpb->getBufferLength(status), tpb->getBuffer(status));
            tpb->dispose();
        } catch (...) { tpb->dispose(); throw; }
    });
}

extern "C" void fbfast_rollback(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        require(!c->cursor && !c->blob, "Close BLOB and cursor before rollback");
        if (c->transaction) { c->transaction->rollback(status); c->transaction = nullptr; }
    });
}

// ALTER SESSION RESET inside the current transaction: Firebird rolls it back,
// resets the session and starts a new transaction with the same parameters,
// all in one round trip. The connection is left with that fresh transaction.
extern "C" uint32_t fbfast_reset_restart(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    uint32_t same = 0;
    guarded(c, error, [&](auto* status) {
        require(c->transaction && !c->cursor && !c->blob,
            "Session restart requires an active transaction and no open cursor");
        ITransaction* next = c->attachment->execute(status, c->transaction, 0,
            "ALTER SESSION RESET", 3, nullptr, nullptr, nullptr, nullptr);
        require(next != nullptr, "Session reset did not restart the transaction");
        same = next == c->transaction ? 1 : 0;
        c->transaction = next;
    });
    return same;
}
