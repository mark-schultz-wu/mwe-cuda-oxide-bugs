# Patch B residual risk / open questions

`patch-b-cross-file.diff` is a **candidate** fix, not a verified one.
It adds a `RigidTy::Str → MirSliceType<u8>` arm to `translate_type` and
`translate_pointer_like` in `crates/mir-importer/src/translator/types.rs`,
on the theory that `&str` and `str` (the unsized pointee) are layout-
identical to `&[u8]` and `[u8]`, so reusing the existing slice path is
sound for translation purposes.

## Why this may be sufficient

- The error fires in the importer's type translator
  (`crates/mir-importer/src/translator/types.rs:706` → the catch-all
  `Type translation not yet implemented for: {:?}` arm). Adding an
  explicit `Str` arm short-circuits that.
- The runtime layout match (fat pointer = data ptr + len) means
  downstream `dialect-mir` operations that touch the slice metadata
  (length comparisons, etc.) keep working without further changes.
- The collector already filters `::panicking::` and `::fmt::` functions
  (`crates/rustc-codegen-cuda/src/collector.rs:1053`), so the runtime
  string-contents are never read; only the *types* need a mapping.

## Why it may not be sufficient

- The kernel-entry path apparently never reaches this code path for the
  same `DisjointSlice::get_mut` bounds-check `&str`. That means either
  (a) MIR-level optimization (DCE / `inline` MIR pass) strips the
  `panic_bounds_check` call site before importer translation runs on
  kernel-entry bodies, or (b) the kernel-entry path uses a different
  importer entrypoint that doesn't hit this `translate_type` arm.
  Without knowing which, the fix may simply move the failure deeper
  (e.g. into `dialect-llvm` codegen of a now-translatable but
  unsupported call).
- `RigidTy::Str` may show up in additional positions the patch doesn't
  cover — e.g. as a constant operand in an rvalue, or as part of a
  larger ADT field. Each such site needs its own check.
- The downstream PTX backend may not know how to materialize a
  `MirSliceType<u8>` value containing a `&'static str` global address.
  If so, the failure moves from translation-time to codegen-time.

## What to verify, in order

1. **Apply both patches, rebuild bug-b-cross-file.** If it still fails:
   capture the new error and check whether it's earlier or later than
   `RigidTy(Str)`. A later failure = patch landed correctly but
   uncovers the next layer; an unchanged failure = the `Str` arm wasn't
   actually exercised (different code path).
2. **Diff the kernel-entry and device-fn importer entrypoints.** Find
   the call sites that drive translation for each:
   - Kernel-entry: `crates/rustc-codegen-cuda/src/device_codegen.rs`
     and `crates/mir-lower/src/lowering.rs` (`is_kernel = true` branch).
   - Device-fn: same files, `is_kernel = false` branch.
   The split likely lives in `device_codegen.rs:298` (the
   `if func.is_kernel { "kernel" } else { "device" }` site, line found
   via grep). Trace each branch through to where `translate_type` is
   ultimately called, and identify which MIR optimizations run between.
3. **Check whether `optimized_mir` vs `mir_for_ctfe` is the source.**
   If the kernel-entry path reads `optimized_mir` (which has DCE) and
   the device-fn path reads pre-optimization MIR, switching the
   device-fn path to `optimized_mir` may strip the panic branches
   uniformly.
4. **If (2) shows the device-fn path skips a MIR pass the kernel-entry
   path runs (e.g. `inline` or `simplify_cfg`),** the structural fix is
   to run those passes on device-fn bodies too, not to keep adding
   one-off type arms.

## Recommendation

Land Patch A (clean and isolated). Apply the Patch B candidate as a
*probe*, not a fix: it costs nothing to leave the `Str` arm in (the
mapping is semantically correct), and it converts an "unknown
unknown" into a "known next failure point" that can be debugged
incrementally. The real fix is almost certainly making the device-fn
import path run the same DCE that the kernel-entry path benefits
from — Patch B's diff just lets us see past the first crash.

## Effort estimate for the structural fix

- ~1 day to instrument both lowering paths and produce a side-by-side
  diff of MIR pass ordering.
- ~1–2 days to unify the paths (or move device-fn import to read
  `optimized_mir`) and shake out the resulting fallout in
  `crates/rustc-codegen-cuda/examples/`.
- Regression coverage: add a `cross_file_device_fn` example mirroring
  the bug-b MWE; place it in `crates/rustc-codegen-cuda/examples/`
  next to `helper_fn/`.
