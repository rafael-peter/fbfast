#include "state.h"
using ControlRef = std::shared_ptr<ControlState>;

extern "C" void* fbfast_control_acquire(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    ControlRef* reference = nullptr;
    guarded(c, error, [&](auto*) { reference = new ControlRef(c->control); });
    return reference;
}

extern "C" void fbfast_control_release(void* handle) noexcept { delete static_cast<ControlRef*>(handle); }

extern "C" void fbfast_control_begin(void* handle, uint64_t generation, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto*) {
        auto& control = *c->control;
        std::lock_guard<std::mutex> lock(control.gate);
        require(generation && !control.generation && control.owner == c, "Invalid control generation");
        control.generation = generation; control.raised = false;
    });
}

extern "C" uint32_t fbfast_control_end(void* handle, uint64_t generation, FbError* error) noexcept {
    auto& control = **static_cast<ControlRef*>(handle);
    std::memset(error, 0, sizeof(*error));
    std::lock_guard<std::mutex> lock(control.gate);
    if (!control.owner || control.generation != generation) return 0;
    control.generation = 0;
    return control.raised;
}

extern "C" uint32_t fbfast_control_raise(void* handle, uint64_t generation, FbError* error) noexcept {
    auto& control = **static_cast<ControlRef*>(handle);
    std::memset(error, 0, sizeof(*error));
    // A stale command never touches an attachment or the next operation.
    std::lock_guard<std::mutex> lock(control.gate);
    if (!control.owner || control.generation != generation || control.raised) return 0;
    control.raised = true;
    guarded(control.owner, error, [&](auto* status) {
        control.owner->attachment->cancelOperation(status, fb_cancel_raise);
    });
    return 1;
}

extern "C" uint32_t fbfast_control_invalidate(void* handle) noexcept {
    return static_cast<Connection*>(handle)->control->invalidate();
}

extern "C" void fbfast_cancel_disable(void* handle, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) { c->attachment->cancelOperation(status, fb_cancel_disable); });
}
