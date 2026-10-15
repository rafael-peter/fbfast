#include <firebird/Interface.h>
#include <cstddef>
#include <cstring>

// The master and its borrowed utility/config interfaces live as long as fbclient.
// Only this translation unit knows the official C++ interface layout.
extern "C" unsigned fbfast_client_version(void* master) noexcept {
    return static_cast<Firebird::IMaster*>(master)->getUtilInterface()->getClientVersion();
}

extern "C" void fbfast_client_root(void* master, char* out, size_t size) noexcept {
    const char* root = static_cast<Firebird::IMaster*>(master)
        ->getConfigManager()->getRootDirectory();
    if (size && root) {
        std::strncpy(out, root, size - 1);
        out[size - 1] = '\0';
    } else if (size) {
        out[0] = '\0';
    }
}

