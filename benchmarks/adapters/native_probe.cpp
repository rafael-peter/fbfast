// Diagnostic reference over exactly the bundled fbclient. This is not the
// library executor or a benchmark equivalent to Python/Arrow materialization.
#include <firebird/Interface.h>
#include <dlfcn.h>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include <vector>

using namespace Firebird;
using Clock = std::chrono::steady_clock;

static long long elapsed(Clock::time_point start) {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - start).count();
}

static const char* required(const char* name) {
    const char* value = std::getenv(name);
    if (!value || !*value) throw std::runtime_error("Missing fixture environment variable");
    return value;
}

struct Probe {
    IMaster* master;
    ThrowStatusWrapper status;
    IProvider* provider;
    IAttachment* attachment = nullptr;
    ITransaction* transaction = nullptr;
    IStatement* statement = nullptr;
    IResultSet* cursor = nullptr;
    IMessageMetadata* metadata = nullptr;

    explicit Probe(IMaster* m) : master(m), status(m->getStatus()), provider(m->getDispatcher()) {}

    // This executable owns its dedicated thread; production uses bounded workers.
    ~Probe() {
        if (cursor) cursor->release();
        if (metadata) metadata->release();
        if (statement) statement->release();
        if (transaction) transaction->release();
        if (attachment) attachment->release();
        provider->release();
        status.dispose();
    }

    void attach() {
        auto* dpb = master->getUtilInterface()->getXpbBuilder(&status, IXpbBuilder::DPB, nullptr, 0);
        try {
            dpb->insertString(&status, isc_dpb_user_name, required("FBFAST_PROBE_USER"));
            dpb->insertString(&status, isc_dpb_password, required("FBFAST_PROBE_PASSWORD"));
            dpb->insertString(&status, isc_dpb_sql_role_name, required("FBFAST_PROBE_ROLE"));
            dpb->insertString(&status, isc_dpb_lc_ctype, "UTF8");
            dpb->insertString(&status, isc_dpb_config, "WireCrypt=Required\nProviders=Remote");
            dpb->insertInt(&status, isc_dpb_connect_timeout, 5);
            attachment = provider->attachDatabase(&status, required("FBFAST_PROBE_DSN"),
                dpb->getBufferLength(&status), dpb->getBuffer(&status));
            dpb->dispose();
        } catch (...) { dpb->dispose(); throw; }
    }

    void sample(int iteration, bool cache) {
        const auto started = Clock::now();
        // Official TPB constants; lock timeout is a length-prefixed LE integer.
        const unsigned char tpb[] = {isc_tpb_version3, isc_tpb_read,
            isc_tpb_read_committed, isc_tpb_read_consistency,
            isc_tpb_wait, isc_tpb_lock_timeout, 4, 5, 0, 0, 0};
        transaction = attachment->startTransaction(&status, sizeof(tpb), tpb);
        const auto tx_ns = elapsed(started);
        const auto preparing = Clock::now();
        if (!statement) {
            const char* sql = "SELECT CAST(MON$READ_ONLY AS INTEGER), CAST(MON$ISOLATION_MODE AS INTEGER) "
                "FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION";
            statement = attachment->prepare(&status, transaction, 0, sql, 3,
                IStatement::PREPARE_PREFETCH_METADATA);
            if (statement->getType(&status) != isc_info_sql_stmt_select)
                throw std::runtime_error("Not a SELECT");
            metadata = statement->getOutputMetadata(&status);
        }
        const auto prepare_ns = elapsed(preparing);
        const auto executing = Clock::now();
        cursor = statement->openCursor(&status, transaction, nullptr, nullptr, metadata, 0);
        const auto execute_ns = elapsed(executing);
        std::vector<std::uint64_t> row((metadata->getMessageLength(&status) + 7) / 8);
        const auto fetching = Clock::now();
        if (cursor->fetchNext(&status, row.data()) != IStatus::RESULT_OK)
            throw std::runtime_error("Missing transaction monitoring row");
        const auto fetch_ns = elapsed(fetching);
        int values[2];
        for (unsigned i = 0; i < 2; ++i) {
            if ((metadata->getType(&status, i) & ~1u) != SQL_LONG)
                throw std::runtime_error("Unexpected monitoring column type");
            const auto* bytes = reinterpret_cast<const unsigned char*>(row.data());
            short null_flag;
            std::memcpy(&null_flag, bytes + metadata->getNullOffset(&status, i), sizeof(null_flag));
            if (null_flag) throw std::runtime_error("NULL transaction metadata");
            std::memcpy(&values[i], bytes + metadata->getOffset(&status, i), sizeof(values[i]));
        }
        if (values[0] != 1 || values[1] != 4)
            throw std::runtime_error("Effective transaction is not READ ONLY READ CONSISTENCY");
        const auto cleaning = Clock::now();
        cursor->close(&status); cursor = nullptr;
        if (!cache) {
            metadata->release(); metadata = nullptr;
            statement->free(&status); statement = nullptr;
        }
        transaction->rollback(&status); transaction = nullptr;
        auto* unexpected = attachment->execute(&status, nullptr, 0, "ALTER SESSION RESET", 3,
            nullptr, nullptr, nullptr, nullptr);
        if (unexpected) {
            unexpected->rollback(&status);
            throw std::runtime_error("Reset unexpectedly opened a transaction");
        }
        const auto cleanup_ns = elapsed(cleaning);
        const auto total_ns = elapsed(started);
        std::cout << iteration << ',' << cache << ',' << tx_ns << ',' << prepare_ns << ','
                  << execute_ns << ',' << fetch_ns << ',' << cleanup_ns << ',' << total_ns << '\n';
    }

    void finish() {
        if (metadata) { metadata->release(); metadata = nullptr; }
        if (statement) { statement->free(&status); statement = nullptr; }
        attachment->detach(&status); attachment = nullptr;
    }
};

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    void* library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!library) { std::cerr << dlerror() << '\n'; return 1; }
    auto get_master = reinterpret_cast<IMaster*(*)()>(dlsym(library, "fb_get_master_interface"));
    if (!get_master) return 2;
    IMaster* master = get_master();
    try {
        Probe probe(master);
        probe.attach();
        std::cout << "sample,cache,transaction_ns,prepare_ns,execute_ns,fetch_ns,cleanup_ns,total_ns\n";
        for (int i = 0; i < 30; ++i) probe.sample(i, false);
        for (int i = 0; i < 30; ++i) probe.sample(i, true);
        probe.finish();
    } catch (const FbException& error) {
        char message[2048];
        master->getUtilInterface()->formatStatus(message, sizeof(message), error.getStatus());
        std::cerr << message << '\n'; return 1;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n'; return 1;
    }
    // The client's process-wide interfaces intentionally stay loaded until exit.
    return 0;
}
