#pragma once
#include <cstddef>
#include <cstdint>

extern "C" {
using SqlStateFn = void (*)(char*, const intptr_t*);
struct FbError {
    int32_t category; // 0 success, 1 Firebird status, 2 bridge invariant, 3 allocation
    uint32_t kind; // 0 other, 1 cancelled, 2 statement timeout, 3 nothing to cancel
    uint32_t code_count;
    int64_t codes[64];
    char sqlstate[6];
    char message[4096];
};
struct FbAttach {
    const char* database;
    const char* user;
    const char* password;
    const char* role;
    const char* charset;
    const char* time_zone;
    const char* app_name;
    const char* config;
    uint32_t connect_seconds;
};
struct FbColumn {
    uint32_t type, length, charset, offset, null_offset;
    int32_t scale, subtype;
    char name[256];
};
struct FbZoned {
    int32_t utc_date;
    uint32_t utc_time;
    int32_t local_date;
    uint32_t local_time;
    char zone[128];
};
void* fbfast_attach(void*, SqlStateFn, const FbAttach*, FbError*) noexcept;
void fbfast_disconnect(void*, FbError*) noexcept;
void fbfast_release(void*) noexcept;
void* fbfast_control_acquire(void*, FbError*) noexcept;
void fbfast_control_release(void*) noexcept;
void fbfast_control_begin(void*, uint64_t, FbError*) noexcept;
uint32_t fbfast_control_end(void*, uint64_t, FbError*) noexcept;
uint32_t fbfast_control_raise(void*, uint64_t, FbError*) noexcept;
uint32_t fbfast_control_invalidate(void*) noexcept;
void fbfast_cancel_disable(void*, FbError*) noexcept;
void fbfast_begin(void*, uint32_t snapshot, uint32_t lock_seconds, uint64_t snapshot_number, FbError*) noexcept;
void fbfast_rollback(void*, FbError*) noexcept;
void fbfast_reset(void*, FbError*) noexcept;
void fbfast_ping(void*, FbError*) noexcept;
uint64_t fbfast_prepare(void*, const char*, FbError*) noexcept;
void fbfast_free_statement(void*, uint64_t, FbError*) noexcept;
uint32_t fbfast_message_length(void*, uint64_t, uint32_t input, FbError*) noexcept;
uint32_t fbfast_column_count(void*, uint64_t, uint32_t input, FbError*) noexcept;
void fbfast_column(void*, uint64_t, uint32_t input, uint32_t index, FbColumn*, FbError*) noexcept;
void fbfast_plan(void*, uint64_t, char*, uint32_t capacity, uint32_t* required, FbError*) noexcept;
void fbfast_open_cursor(void*, uint64_t, const void*, uint32_t length, uint32_t timeout_ms, FbError*) noexcept;
uint32_t fbfast_fetch(void*, void*, uint32_t stride, uint32_t capacity, uint32_t* eof, FbError*) noexcept;
void fbfast_close_cursor(void*, FbError*) noexcept;
void fbfast_open_blob(void*, const uint8_t* id, uint32_t charset, FbError*) noexcept;
uint32_t fbfast_read_blob(void*, void*, uint32_t capacity, uint32_t* eof, FbError*) noexcept;
void fbfast_close_blob(void*, FbError*) noexcept;
void fbfast_decimal_text(void*, uint32_t type, const uint8_t*, char*, uint32_t capacity, FbError*) noexcept;
void fbfast_decimal_bytes(void*, uint32_t type, const char*, uint8_t*, FbError*) noexcept;
void fbfast_decode_zoned(void*, uint32_t type, const uint8_t*, FbZoned*, FbError*) noexcept;
void fbfast_encode_zoned(void*, uint32_t type, int32_t date, uint32_t time, const char*, uint8_t*, FbError*) noexcept;
}
