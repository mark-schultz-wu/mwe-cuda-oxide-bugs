# Writeup

## Bug A — non-unit tuple return coerced to void

`mir-lower/src/convert/ops/call.rs:370` reads
`let is_unit = mir_ty.deref(ctx).is::<MirTupleType>();`. The variable
name suggests a unit-type check, but `is::<MirTupleType>()` matches
*every* tuple, including 2-tuples like `(u64, u64)`. The next branch
coerces the call's result to `Void`, which then drives the
`replace_operation` vs `erase_operation` choice at lines 410–413 into
the erase branch. The original MIR call op's result is still live at
the destructuring use site (`let (lo, hi) = helper(…)`), so
`Operation::erase` (`pliron/src/operation.rs:526`) panics on the
live-use assertion. Fix: check `MirTupleType::get_types().is_empty()`
to distinguish real `()` from non-empty tuples; non-empty tuples then
take the `convert_type` path that already builds an unnamed LLVM
struct for them.

## Bug B — cross-file device helper triggers `RigidTy(Str)`

`mir-importer/src/translator/types.rs` translates Rust types to
`dialect-mir` types. There is no arm for `RigidTy::Str`; the unsized
`str` pointee inside `&'static str` panic-message globals reachable
from `DisjointSlice::get_mut`'s bounds check falls through to the
catch-all "Type translation not yet implemented for: {:?}" error.
When the helper is inlined directly in the kernel body, this code
path apparently isn't hit (likely because the kernel-entry MIR has
already been DCE-d or routed through a different translator entry).
When the helper lives in a sibling file, the device-function lowering
path imports the helper's pre-optimization MIR and the type
translator chokes on the `&str` arg of the still-present (unreachable)
`panic_bounds_check`. The candidate patch adds a `Str` arm that maps
to `MirSliceType<u8>` (layout-identical fat pointer); this may be
sufficient (the values are never read at runtime, only typed), or it
may just expose the next failure point downstream. See
`patches/patch-b-cross-file-PLAN.md` for the unverified surface and
the deeper structural fix (unify device-fn and kernel-entry import on
the optimized-MIR path).

## What's still open

- **Patch A**: verified to apply cleanly against
  cuda-oxide `main@c8b3103`. Runtime verification (build with the
  MWE, observe panic, apply patch, observe success) was not done
  locally — no `cargo-oxide` install, no CUDA toolchain.
- **Patch B**: candidate fix only. The deeper question — why the
  kernel-entry path tolerates the same `&str` types — is documented
  in the PLAN companion but not investigated to ground truth. The
  recommendation is to land Patch B as a probe and treat any
  subsequent error as the real next bug, not as evidence the patch
  was wrong.
- **Regression tests**: cuda-oxide's expected-failure pattern is the
  `crates/rustc-codegen-cuda/examples/error_*` directories. Tests
  weren't added in this MWE because (a) the prompt explicitly said
  they aren't required and (b) without a CUDA build environment I
  couldn't confirm they'd be wired into the harness correctly. Both
  bug crates are minimal enough to drop into
  `crates/rustc-codegen-cuda/examples/` more or less as-is — bug A
  as a normal `examples/helper_fn_tuple_return/`, bug B as a normal
  `examples/cross_file_device_fn/`.
