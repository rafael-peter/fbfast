#include "state.h"

static_assert(sizeof(ISC_TIME_TZ) == 8);
static_assert(sizeof(ISC_TIMESTAMP_TZ) == 12);

extern "C" void fbfast_decode_zoned(void* handle, uint32_t type,
    const uint8_t* data, FbZoned* out, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        std::memset(out, 0, sizeof(*out));
        auto* util = c->master->getUtilInterface();
        unsigned year = 0, month = 0, day = 0, hour, minute, second, fraction;
        if (type == SQL_TIME_TZ) {
            ISC_TIME_TZ value;
            std::memcpy(&value, data, sizeof(value));
            out->utc_time = value.utc_time;
            util->decodeTimeTz(status, &value, &hour, &minute, &second, &fraction,
                sizeof(out->zone), out->zone);
        } else {
            require(type == SQL_TIMESTAMP_TZ, "Unsupported zoned storage type");
            ISC_TIMESTAMP_TZ value;
            std::memcpy(&value, data, sizeof(value));
            out->utc_date = value.utc_timestamp.timestamp_date;
            out->utc_time = value.utc_timestamp.timestamp_time;
            util->decodeTimeStampTz(status, &value, &year, &month, &day,
                &hour, &minute, &second, &fraction, sizeof(out->zone), out->zone);
            out->local_date = util->encodeDate(year, month, day);
        }
        out->local_time = util->encodeTime(hour, minute, second, fraction);
    });
}

extern "C" void fbfast_encode_zoned(void* handle, uint32_t type,
    int32_t date, uint32_t time, const char* zone, uint8_t* out, FbError* error) noexcept {
    auto* c = static_cast<Connection*>(handle);
    guarded(c, error, [&](auto* status) {
        // Resolve the zone with the official codec. Store the caller's UTC
        // instant directly: encoding ambiguous local time would lose DST fold.
        ISC_TIME_TZ parsed{};
        std::memset(&parsed, 0, sizeof(parsed));
        c->master->getUtilInterface()->encodeTimeTz(status, &parsed, 12, 0, 0, 0, zone);
        if (type == SQL_TIME_TZ) {
            parsed.utc_time = time;
            std::memcpy(out, &parsed, sizeof(parsed));
        } else {
            require(type == SQL_TIMESTAMP_TZ, "Unsupported zoned storage type");
            ISC_TIMESTAMP_TZ value{};
            std::memset(&value, 0, sizeof(value));
            value.utc_timestamp.timestamp_date = date;
            value.utc_timestamp.timestamp_time = time;
            value.time_zone = parsed.time_zone;
            std::memcpy(out, &value, sizeof(value));
        }
    });
}
