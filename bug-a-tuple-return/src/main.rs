//! Bug A reproducer: a `#[device]` helper returning `(u64, u64)` panics
//! cuda-oxide's mir-lower in `Operation::erase` ("Operation with use(s)
//! being erased"). Root cause is the `is_unit = ty.is::<MirTupleType>()`
//! mis-naming at `mir-lower/src/convert/ops/call.rs:370` — ANY tuple
//! gets coerced to `Void`, then the call op is erased while its result
//! is still referenced by the destructuring `let (lo, hi) = ...`.
//!
//! To toggle the bug off (verifies it's the tuple type and nothing else),
//! flip `USE_TUPLE` to `false` — the helper returns `[u64; 2]` instead,
//! arrays take the correct `convert_type` path, build succeeds.

use cuda_device::{DisjointSlice, device, kernel, thread};
use cuda_host::cuda_module;

const USE_TUPLE: bool = true;

#[device]
pub fn split_u128_tuple(x: u64) -> (u64, u64) {
    (x.wrapping_mul(3), x.wrapping_add(7))
}

#[device]
pub fn split_u128_array(x: u64) -> [u64; 2] {
    [x.wrapping_mul(3), x.wrapping_add(7)]
}

#[cuda_module]
mod kernels {
    use super::*;

    #[kernel]
    pub fn bug_a(input: &[u64], mut out: DisjointSlice<u64>) {
        let idx = thread::index_1d();
        let i = idx.get();
        if let Some(slot) = out.get_mut(idx) {
            if USE_TUPLE {
                // BUG: helper returns (u64, u64). Build panics in pliron's
                // Operation::erase during mir_lower::convert::ops::call::convert.
                let (lo, hi) = split_u128_tuple(input[i]);
                *slot = lo ^ hi;
            } else {
                // OK: array return — convert_type handles it correctly.
                let arr = split_u128_array(input[i]);
                *slot = arr[0] ^ arr[1];
            }
        }
    }
}

fn main() {}
