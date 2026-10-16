//! All declarations mirror bridge/bridge.h; no C++ layout escapes that bridge.
use std::ffi::{c_char, c_void};

pub type MasterFn = unsafe extern "C" fn() -> *mut c_void;
pub type SqlStateFn = unsafe extern "C" fn(*mut c_char, *const isize);

#[repr(C)]
pub struct RawStatus {
    pub category: i32,
    pub kind: u32,
    pub code_count: u32,
    pub codes: [i64; 64],
    pub sqlstate: [c_char; 6],
    pub message: [c_char; 4096],
}

impl Default for RawStatus {
    fn default() -> Self {
        Self {
            category: 0,
            kind: 0,
            code_count: 0,
            codes: [0; 64],
            sqlstate: [0; 6],
            message: [0; 4096],
        }
    }
}

#[repr(C)]
pub struct RawAttach {
    pub database: *const c_char,
    pub user: *const c_char,
    pub password: *const c_char,
    pub role: *const c_char,
    pub charset: *const c_char,
    pub time_zone: *const c_char,
    pub app_name: *const c_char,
    pub config: *const c_char,
    pub connect_seconds: u32,
}

#[repr(C)]
pub struct RawColumn {
    pub sql_type: u32,
    pub length: u32,
    pub charset: u32,
    pub offset: u32,
    pub null_offset: u32,
    pub scale: i32,
    pub subtype: i32,
    pub name: [c_char; 256],
}

impl Default for RawColumn {
    fn default() -> Self {
        Self {
            sql_type: 0,
            length: 0,
            charset: 0,
            offset: 0,
            null_offset: 0,
            scale: 0,
            subtype: 0,
            name: [0; 256],
        }
    }
}

#[repr(C)]
pub struct RawZoned {
    pub utc_date: i32,
    pub utc_time: u32,
    pub local_date: i32,
    pub local_time: u32,
    pub zone: [u8; 128],
}

unsafe extern "C" {
    pub fn fbfast_decode_zoned(
        handle: *mut c_void,
        kind: u32,
        data: *const u8,
        out: *mut RawZoned,
        error: *mut RawStatus,
    );
    pub fn fbfast_encode_zoned(
        handle: *mut c_void,
        kind: u32,
        date: i32,
        time: u32,
        zone: *const c_char,
        out: *mut u8,
        error: *mut RawStatus,
    );
    pub fn fbfast_attach(
        master: *mut c_void,
        sqlstate: SqlStateFn,
        options: *const RawAttach,
        error: *mut RawStatus,
    ) -> *mut c_void;
    pub fn fbfast_disconnect(handle: *mut c_void, error: *mut RawStatus);
    pub fn fbfast_release(handle: *mut c_void);
    pub fn fbfast_control_acquire(handle: *mut c_void, error: *mut RawStatus) -> *mut c_void;
    pub fn fbfast_control_release(handle: *mut c_void);
    pub fn fbfast_control_begin(handle: *mut c_void, generation: u64, error: *mut RawStatus);
    pub fn fbfast_control_end(handle: *mut c_void, generation: u64, error: *mut RawStatus) -> u32;
    pub fn fbfast_control_raise(handle: *mut c_void, generation: u64, error: *mut RawStatus)
        -> u32;
    pub fn fbfast_control_invalidate(handle: *mut c_void) -> u32;
    pub fn fbfast_cancel_disable(handle: *mut c_void, error: *mut RawStatus);
    pub fn fbfast_begin(
        handle: *mut c_void,
        snapshot: u32,
        lock_seconds: u32,
        snapshot_number: u64,
        error: *mut RawStatus,
    );
    pub fn fbfast_rollback(handle: *mut c_void, error: *mut RawStatus);
    pub fn fbfast_reset(handle: *mut c_void, error: *mut RawStatus);
    pub fn fbfast_ping(handle: *mut c_void, error: *mut RawStatus);
    pub fn fbfast_prepare(handle: *mut c_void, sql: *const c_char, error: *mut RawStatus) -> u64;
    pub fn fbfast_free_statement(handle: *mut c_void, id: u64, error: *mut RawStatus);
    pub fn fbfast_message_length(
        handle: *mut c_void,
        id: u64,
        input: u32,
        error: *mut RawStatus,
    ) -> u32;
    pub fn fbfast_column_count(
        handle: *mut c_void,
        id: u64,
        input: u32,
        error: *mut RawStatus,
    ) -> u32;
    pub fn fbfast_column(
        handle: *mut c_void,
        id: u64,
        input: u32,
        index: u32,
        out: *mut RawColumn,
        error: *mut RawStatus,
    );
    pub fn fbfast_plan(
        handle: *mut c_void,
        id: u64,
        out: *mut c_char,
        capacity: u32,
        required: *mut u32,
        error: *mut RawStatus,
    );
    pub fn fbfast_open_cursor(
        handle: *mut c_void,
        id: u64,
        input: *const c_void,
        length: u32,
        timeout_ms: u32,
        error: *mut RawStatus,
    );
    pub fn fbfast_fetch(
        handle: *mut c_void,
        out: *mut c_void,
        stride: u32,
        capacity: u32,
        eof: *mut u32,
        error: *mut RawStatus,
    ) -> u32;
    pub fn fbfast_close_cursor(handle: *mut c_void, error: *mut RawStatus);
    pub fn fbfast_open_blob(
        handle: *mut c_void,
        id: *const u8,
        charset: u32,
        error: *mut RawStatus,
    );
    pub fn fbfast_read_blob(
        handle: *mut c_void,
        out: *mut c_void,
        capacity: u32,
        eof: *mut u32,
        error: *mut RawStatus,
    ) -> u32;
    pub fn fbfast_close_blob(handle: *mut c_void, error: *mut RawStatus);
    pub fn fbfast_decimal_text(
        handle: *mut c_void,
        kind: u32,
        data: *const u8,
        text: *mut c_char,
        capacity: u32,
        error: *mut RawStatus,
    );
    pub fn fbfast_decimal_bytes(
        handle: *mut c_void,
        kind: u32,
        text: *const c_char,
        data: *mut u8,
        error: *mut RawStatus,
    );
}
