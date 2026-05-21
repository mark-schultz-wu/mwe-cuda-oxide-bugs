//! Bug B reproducer: a kernel whose body just calls an `#[inline(always)]
//! pub fn` helper defined in a sibling source file fails PTX codegen with
//! `Type translation not yet implemented for: RigidTy(Str)`.
//!
//! The `&str` originates from `DisjointSlice::get_mut`'s panic-message
//! globals; the kernel-entry lowering path tolerates / strips those, but
//! the device-function path triggered by a cross-file call does not.
//!
//! To toggle the bug off (verifies it's the cross-file split and nothing
//! else), set `INLINE_HELPER` to `true` — same body inlined directly in
//! the kernel builds and produces valid PTX.

mod helpers;

use cuda_device::{DisjointSlice, kernel, thread};
use cuda_host::cuda_module;

// Read at compile time from env. Default: bug triggers.
// Run `INLINE=1 cargo oxide build --arch sm_89` for the OK (inlined) variant.
// build.rs declares rerun-if-env-changed=INLINE so no `cargo clean` is needed.
const INLINE_HELPER: bool = option_env!("INLINE").is_some();

#[cuda_module]
mod kernels {
    use super::*;

    #[kernel]
    pub fn bug_b(a: &[u64], b: &[u64], mut out: DisjointSlice<u64>) {
        if INLINE_HELPER {
            // OK: body inlined directly in the kernel → kernel-entry
            // lowering path → str panic-message types are tolerated.
            let idx = thread::index_1d();
            if let Some(slot) = out.get_mut(idx) {
                *slot = 42;
            }
        } else {
            // BUG: helper lives in sibling file, takes device-fn path.
            // Build fails with RigidTy(Str).
            crate::helpers::foo_impl(out, a, b, 0);
        }
    }
}

fn main() {}
