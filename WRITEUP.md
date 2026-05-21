# Writeup

## Runtime verification

Both bug reproducers and the patches were verified on a shared GPU
host (CUDA 12.6, nightly-2026-04-03, cuda-oxide built from
c8b3103cb1ffe1664aa1458b467ebc2206d5d82a).

| Crate                | Unpatched                                          | Patched (A + B)                                          |
|----------------------|----------------------------------------------------|----------------------------------------------------------|
| `bug-a-tuple-return` | rustc ICE: `Operation with use(s) being erased`    | **Build succeeds**                                       |
| `bug-b-cross-file`   | `Type translation not yet implemented for: RigidTy(Str)` | New error: `Unsupported constant type in translate_constant` (next layer) |

Procedure: cloned cuda-oxide at c8b3103, applied both patches, built
`crates/rustc-codegen-cuda` (release; needed `RUSTFLAGS="-L native=$(rustc --print sysroot)/lib"`
to locate `libLLVM-22-rust-1.96.0-nightly.so` and `CUDA_OXIDE_LLC` to
point at the rustup `llvm-tools` `llc`), then swapped the resulting
`librustc_codegen_cuda.so` into `~/.cargo/cuda-oxide/`, ran each MWE
build, and restored the original `.so` afterward.

## Bug A — non-unit tuple return coerced to void

`mir-lower/src/convert/ops/call.rs:370` read
`let is_unit = mir_ty.deref(ctx).is::<MirTupleType>();`. The variable
name suggested a unit-type check, but `is::<MirTupleType>()` matched
*every* tuple, including 2-tuples like `(u64, u64)`. The next branch
coerced the call's result to `Void`, which then drove the
`replace_operation` vs `erase_operation` choice at lines 410-413 into
the erase branch. The original MIR call op's result was still live at
the destructuring use site (`let (lo, hi) = helper(...)`), so
`Operation::erase` (`pliron/src/operation.rs:526`) panicked on the
live-use assertion. **Fix**: check
`MirTupleType::get_types().is_empty()` to distinguish real `()` from
non-empty tuples; non-empty tuples then take the `convert_type` path
that already builds an unnamed LLVM struct for them. Verified to
clear the panic and produce a successful PTX build on the MWE.

## Bug B — cross-file device helper triggers RigidTy(Str)

`mir-importer/src/translator/types.rs` translates Rust types to
`dialect-mir` types. There was no arm for `RigidTy::Str`; the unsized
`str` pointee inside `&'static str` panic-message globals reachable
from `DisjointSlice::get_mut`'s bounds check fell through to the
catch-all "Type translation not yet implemented for: {:?}" error.
When the helper is inlined directly in the kernel body, this code
path apparently isn't hit (likely because the kernel-entry MIR has
already been DCE-d or routed through a different translator entry).
When the helper lives in a sibling file, the device-function lowering
path imports the helper's pre-optimization MIR and the type
translator chokes on the `&str` arg of the still-present (unreachable)
`panic_bounds_check`.

**Patch attempt** (`patches/patch-b-cross-file.diff`): added a `Str`
arm that maps to `MirSliceType<u8>` (layout-identical fat pointer) in
both `translate_type` and `translate_pointer_like`. **Verified to
clear the original "Type translation not yet implemented" error** but
revealed a second-layer failure: `translate_constant` in
`crates/mir-importer/src/translator/rvalue.rs:2315` has no handler
for fat-pointer slice constants, so when it tries to materialize the
`&'static str` panic-message constant against the new `MirSliceType`,
its dispatch ladder (ZST -> ptr_to_array -> struct -> enum -> float ->
pointer -> integer) misses entirely.

This confirms the Patch B PLAN's "probe, not fix" framing: the type
arm is necessary but not sufficient. Two follow-ups are visible:

1. **Add a fat-pointer-constant arm to `translate_constant`** that
   emits an aggregate of `(ptr_to_alloc, integer_len)` for slice and
   string-slice constants. This is the surgical fix.
2. **Or, ablate the panic-bounds-check path earlier** so the `&str`
   constant never reaches translation -- i.e., run the same MIR DCE
   on device-fn bodies that already runs on kernel-entry bodies. This
   is the structural fix and the one the PLAN recommends pursuing.

## What's still open

- **Patch A**: done.
- **Patch B**: the type-translation arm lands cleanly and moves the
  failure to a known site (`translate_constant` slice/str constants).
  The deeper structural unification of kernel-entry and device-fn
  import paths remains the recommended follow-up.
- **Regression tests**: cuda-oxide's expected-failure pattern is the
  `crates/rustc-codegen-cuda/examples/error_*` directories. Both bug
  crates are minimal enough to drop into
  `crates/rustc-codegen-cuda/examples/` as-is -- bug A as
  `examples/helper_fn_tuple_return/`, bug B as
  `examples/cross_file_device_fn/` (and once fully fixed, a non-`error_`
  variant alongside).
