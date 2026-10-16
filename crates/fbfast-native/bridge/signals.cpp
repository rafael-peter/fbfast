#include <firebird/Interface.h>
#include <csignal>
#include <cstdint>
#if defined(__unix__) || defined(__APPLE__)
#include <unistd.h>
#endif

namespace {
int host_owns_shutdown(const int reason, const int, void*) noexcept {
    // A handled Ctrl+C must cancel one operation, not shut down the process-wide
    // client. Keep the official exit/unload shutdown path unchanged.
    return reason == fb_shutrsn_signal ? 1 : 0;
}

#if defined(__unix__) || defined(__APPLE__)
void default_termination(int number) noexcept {
    // fbclient chains callable handlers but drops SIG_DFL/SIG_IGN. Preserve
    // default termination through that chain. These are async-signal-safe calls;
    // no Python, allocation, lock, Firebird API or cleanup runs in this handler.
    struct sigaction action {};
    action.sa_handler = SIG_DFL;
    sigemptyset(&action.sa_mask);
    if (sigaction(number, &action, nullptr) != 0 || kill(getpid(), number) != 0)
        _exit(128 + number);
}

bool preserve_default(int number) noexcept {
    struct sigaction action {};
    if (sigaction(number, nullptr, &action) != 0) return false;
    if (action.sa_handler != SIG_DFL) return true;
    action.sa_handler = default_termination;
    action.sa_flags = 0;
    sigemptyset(&action.sa_mask);
    return sigaction(number, &action, nullptr) == 0;
}
#endif
}

extern "C" uint32_t fbfast_client_signals(void* register_callback) noexcept {
    // The signature/constants come from the installed SDK, not a duplicated ABI.
    const auto register_fn = reinterpret_cast<decltype(&fb_shutdown_callback)>(register_callback);
    ISC_STATUS_ARRAY status {};
    if (register_fn(status, host_owns_shutdown, fb_shut_confirmation, nullptr)) return 1;
#if defined(__unix__) || defined(__APPLE__)
    // Install before the first dispatcher entry. fbclient will chain custom
    // application handlers; ignored signals remain ignored by the veto above.
    if (!preserve_default(SIGINT) || !preserve_default(SIGTERM)) return 2;
#endif
    return 0;
}
