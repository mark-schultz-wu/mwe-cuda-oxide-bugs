//! Sibling source file — placement here (vs. inlining in the kernel body)
//! is the whole point of the reproducer. The helper itself is trivial.

use cuda_device::{DisjointSlice, thread};

#[inline(always)]
pub fn foo_impl(mut out: DisjointSlice<u64>, _a: &[u64], _b: &[u64], _p: u64) {
    let idx = thread::index_1d();
    if let Some(slot) = out.get_mut(idx) {
        *slot = 42;
    }
}
