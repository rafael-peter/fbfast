#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fbfast_native::fuzz_metadata(data));
