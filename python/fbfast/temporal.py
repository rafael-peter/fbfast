"""Lossless public temporal types and Python-only datetime conversion."""
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta, timezone
import re
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from ._native import DataConversionError

_MJD_ORDINAL = 678576
_OFFSET = re.compile(r"([+-])(\d{2}):(\d{2})", re.ASCII)


def _error(message: str, phase: str = "python_conversion") -> DataConversionError:
    error = DataConversionError(message)
    error.phase = phase
    error.sqlstate = None
    error.native_codes = []
    error.cleanup_errors = []
    return error


@dataclass(frozen=True, slots=True)
class TimeWithZone:
    """Firebird TIME WITH TIME ZONE without inventing a calendar date.

    Args:
        utc_time: UTC time of day, supplied as a naive datetime.time, with
            100-microsecond precision and fold=0. Required; no default.
        zone: Firebird/IANA zone name or signed HH:MM offset. Required;
            no default. Validated against the bundled client during binding.

    Values are immutable and hashable. The UTC time and zone are retained
    separately; a region's local time cannot be derived without date rules.
    """

    utc_time: time
    zone: str

    def __post_init__(self) -> None:
        if not isinstance(self.utc_time, time) or self.utc_time.tzinfo is not None or self.utc_time.fold:
            raise _error("utc_time must be a naive UTC time with fold=0")
        _ticks(self.utc_time)
        if not isinstance(self.zone, str) or not self.zone or not self.zone.isascii() or "\0" in self.zone or len(self.zone) >= 128:
            raise _error("zone must be a nonempty ASCII name or offset shorter than 128 bytes")


def _ticks(value: time | datetime) -> int:
    if value.microsecond % 100:
        raise _error("Firebird TIME precision is 100 microseconds; value would lose precision")
    return ((value.hour * 60 + value.minute) * 60 + value.second) * 10000 + value.microsecond // 100


def _timestamp_parameter(value: datetime) -> tuple[int, int, int, int, str]:
    """Keep the original wall time so the worker can check the client's rules."""
    tz = value.tzinfo
    if isinstance(tz, ZoneInfo):
        zone = tz.key
        if not isinstance(zone, str) or not zone:
            raise _error("ZoneInfo must have a named key for Firebird binding", "bind")
    elif isinstance(tz, timezone):
        offset = value.utcoffset()
        if offset is None or offset.microseconds or offset.seconds % 60:
            raise _error("Firebird fixed time zone offsets require whole minutes", "bind")
        minutes = offset.days * 1440 + offset.seconds // 60
        if abs(minutes) > 14 * 60:
            raise _error("Firebird time zone offset exceeds 14 hours", "bind")
        zone = f"{'-' if minutes < 0 else '+'}{abs(minutes) // 60:02}:{abs(minutes) % 60:02}"
    else:
        raise _error("Aware datetime requires zoneinfo.ZoneInfo or datetime.timezone", "bind")
    try:
        utc = value.astimezone(timezone.utc)
    except (ValueError, OverflowError) as error:
        raise _error("Timestamp UTC instant is outside the supported date range", "bind") from error
    return (utc.toordinal() - _MJD_ORDINAL, _ticks(utc),
            value.toordinal() - _MJD_ORDINAL, _ticks(value), zone)


def _timestamp_from_native(utc_date: int, utc_time: int, local_date: int, local_time: int, zone: str) -> datetime:
    try:
        day = date.fromordinal(utc_date + _MJD_ORDINAL)
        utc = datetime.combine(day, time(), timezone.utc) + timedelta(microseconds=utc_time * 100)
        match = _OFFSET.fullmatch(zone)
        if match:
            hours, minutes = int(match[2]), int(match[3])
            if minutes > 59 or hours * 60 + minutes > 840:
                raise ValueError("Invalid native offset")
            delta = timedelta(minutes=(hours * 60 + minutes) * (-1 if match[1] == "-" else 1))
            tz: ZoneInfo | timezone = timezone(delta)
        else:
            tz = ZoneInfo(zone)
        local = utc.astimezone(tz)
        if local.toordinal() - _MJD_ORDINAL != local_date or _ticks(local) != local_time:
            raise _error("Python and Firebird time zone rules disagree; use JSON to retain UTC and zone")
        return local
    except (ValueError, OverflowError, ZoneInfoNotFoundError) as error:
        raise _error("Time zone cannot be represented faithfully in Python; use JSON to retain UTC and zone") from error
