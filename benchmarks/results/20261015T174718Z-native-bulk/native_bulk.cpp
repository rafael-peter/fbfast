// Diagnostic native materialization of two int32 columns over the SAME fbclient.
// No Python/JSON/Arrow conversion. Not an equivalent end-user output benchmark.
#include <firebird/Interface.h>
#include <dlfcn.h>
#include <sys/resource.h>
#include <array>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <iostream>
#include <stdexcept>
#include <vector>

using namespace Firebird;
using Clock = std::chrono::steady_clock;

static double seconds(Clock::time_point begin) {
    return std::chrono::duration<double>(Clock::now() - begin).count();
}

static const char* required(const char* name) {
    auto value = std::getenv(name);
    if (!value || !*value) throw std::runtime_error("Missing fixture configuration");
    return value;
}

class Bulk {
    IMaster* master;
    ThrowStatusWrapper status;
    IProvider* provider;
    IAttachment* attachment = nullptr;
    ITransaction* transaction = nullptr;
    IStatement* statement = nullptr;
    IResultSet* cursor = nullptr;
    IMessageMetadata* input = nullptr;
    IMessageMetadata* output = nullptr;
    unsigned offsets[2] = {};
    unsigned nulls[2] = {};
public:
    explicit Bulk(IMaster* m) : master(m), status(m->getStatus()), provider(m->getDispatcher()) {
        auto dpb = master->getUtilInterface()->getXpbBuilder(&status, IXpbBuilder::DPB, nullptr, 0);
        try {
            dpb->insertString(&status, isc_dpb_user_name, required("FBFAST_PROBE_USER"));
            dpb->insertString(&status, isc_dpb_password, required("FBFAST_PROBE_PASSWORD"));
            dpb->insertString(&status, isc_dpb_sql_role_name, required("FBFAST_PROBE_ROLE"));
            dpb->insertString(&status, isc_dpb_lc_ctype, "UTF8");
            dpb->insertString(&status, isc_dpb_config,
                              "WireCrypt=Required\nWireCompression=false\nProviders=Remote");
            attachment = provider->attachDatabase(&status, required("FBFAST_PROBE_DSN"),
                dpb->getBufferLength(&status), dpb->getBuffer(&status));
            dpb->dispose();
        } catch (...) { dpb->dispose(); throw; }
    }
    ~Bulk() {
        if (cursor) cursor->release();
        if (input) input->release();
        if (output) output->release();
        if (statement) statement->release();
        if (transaction) transaction->release();
        if (attachment) attachment->release();
        provider->release();
        status.dispose();
    }
    void sample(int iteration, int count) {
        const auto begin = Clock::now();
        const auto cpu = std::clock();
        const unsigned char tpb[] = {isc_tpb_version3, isc_tpb_read, isc_tpb_read_committed,
            isc_tpb_read_consistency, isc_tpb_wait, isc_tpb_lock_timeout, 4, 5, 0, 0, 0};
        transaction = attachment->startTransaction(&status, sizeof(tpb), tpb);
        if (!statement) {
            statement = attachment->prepare(&status, transaction, 0,
                "SELECT ID, V FROM FBFAST_BENCH WHERE ID <= ? ORDER BY ID", 3,
                IStatement::PREPARE_PREFETCH_METADATA);
            statement->setTimeout(&status, 120000);
            input = statement->getInputMetadata(&status);
            output = statement->getOutputMetadata(&status);
            if (input->getCount(&status) != 1 || output->getCount(&status) != 2 ||
                (input->getType(&status, 0) & ~1u) != SQL_LONG)
                throw std::runtime_error("Unexpected narrow query metadata");
            for (unsigned i = 0; i < 2; ++i) {
                if ((output->getType(&status, i) & ~1u) != SQL_LONG)
                    throw std::runtime_error("Expected int32 output");
                offsets[i] = output->getOffset(&status, i);
                nulls[i] = output->getNullOffset(&status, i);
            }
        }
        std::vector<std::uint64_t> parameter((input->getMessageLength(&status) + 7) / 8, 0);
        std::memcpy(reinterpret_cast<char*>(parameter.data()) + input->getOffset(&status, 0),
                    &count, sizeof(count));
        cursor = statement->openCursor(&status, transaction, input, parameter.data(), output, 0);
        const double open_seconds = seconds(begin);
        std::vector<std::uint64_t> buffer((output->getMessageLength(&status) + 7) / 8);
        std::vector<std::array<std::int32_t, 2>> rows;
        rows.reserve(count);
        const auto fetching = Clock::now();
        double first_seconds = 0;
        bool null_found = false;
        while (cursor->fetchNext(&status, buffer.data()) == IStatus::RESULT_OK) {
            if (rows.empty()) first_seconds = seconds(begin);
            const auto bytes = reinterpret_cast<const char*>(buffer.data());
            std::array<std::int32_t, 2> row;
            for (unsigned i = 0; i < 2; ++i) {
                short flag;
                std::memcpy(&flag, bytes + nulls[i], sizeof(flag));
                null_found |= flag != 0;
                std::memcpy(&row[i], bytes + offsets[i], sizeof(row[i]));
            }
            rows.push_back(row);
        }
        const double fetch_seconds = seconds(fetching);
        const auto cleaning = Clock::now();
        cursor->close(&status); cursor = nullptr;
        transaction->rollback(&status); transaction = nullptr;
        auto unexpected = attachment->execute(&status, nullptr, 0, "ALTER SESSION RESET", 3,
                                               nullptr, nullptr, nullptr, nullptr);
        if (unexpected) {
            unexpected->rollback(&status);
            throw std::runtime_error("RESET returned a transaction");
        }
        const double cleanup_seconds = seconds(cleaning);
        const double elapsed = seconds(begin);
        const double cpu_seconds = double(std::clock() - cpu) / CLOCKS_PER_SEC;
        struct rusage usage;
        if (getrusage(RUSAGE_SELF, &usage)) throw std::runtime_error("getrusage failed");
        // Complete independent validation AFTER timed fetch/cleanup and RSS.
        if (null_found || rows.size() != static_cast<std::size_t>(count))
            throw std::runtime_error("Incorrect result size or NULL");
        std::uint64_t checksum = 14695981039346656037ull;
        for (int i = 1; i <= count; ++i) {
            if (rows[i - 1][0] != i || rows[i - 1][1] != (i * 17 % 100003) - 50001)
                throw std::runtime_error("Incorrect result value or order");
            for (auto value : rows[i - 1]) {
                auto bits = static_cast<std::uint32_t>(value);
                for (int b = 0; b < 4; ++b) {
                    checksum = (checksum ^ ((bits >> (8 * b)) & 255)) * 1099511628211ull;
                }
            }
        }
        if (iteration >= 0)
            std::cout << "{\"sample\":" << iteration << ",\"rows\":" << count
                << ",\"seconds\":" << elapsed << ",\"cpu_seconds\":" << cpu_seconds
                << ",\"open_seconds\":" << open_seconds << ",\"first_row_seconds\":" << first_seconds
                << ",\"fetch_seconds\":" << fetch_seconds << ",\"cleanup_seconds\":" << cleanup_seconds
                << ",\"process_peak_rss_bytes\":" << usage.ru_maxrss
                << ",\"fnv1a64\":\"" << checksum << "\"}" << std::endl;
    }
    void finish() {
        input->release(); input = nullptr;
        output->release(); output = nullptr;
        statement->free(&status); statement = nullptr;
        attachment->detach(&status); attachment = nullptr;
    }
};

int main(int argc, char** argv) {
    if (argc != 4) return 2;
    const int rows = std::atoi(argv[2]), samples = std::atoi(argv[3]);
    if ((rows != 100000 && rows != 1000000) || samples < 1 || samples > 30) return 2;
    void* library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!library) return 3;
    auto get_master = reinterpret_cast<IMaster*(*)()>(dlsym(library, "fb_get_master_interface"));
    if (!get_master) return 3;
    auto master = get_master();
    try {
        Bulk probe(master);
        probe.sample(-1, rows);
        for (int i = 0; i < samples; ++i) probe.sample(i, rows);
        probe.finish();
    } catch (const FbException& error) {
        char message[2048];
        master->getUtilInterface()->formatStatus(message, sizeof(message), error.getStatus());
        std::cerr << message << '\n'; return 1;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n'; return 1;
    }
    return 0;
}
