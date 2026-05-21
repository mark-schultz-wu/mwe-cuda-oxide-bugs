# Patch B residual risk / open questions

`patch-b-cross-file.diff` is a **type-translation probe**, not a full
fix. Runtime verification with both patches applied confirmed:

- The patch *successfully* clears the original error
  `Type translation not yet implemented for: RigidTy(Str)`.
- It then surfaces a second-layer failure in `translate_constant`
  (`crates/mir-importer/src/translator/rvalue.rs:2315`): no constant
  handler exists for a `MirSliceType<u8>` constant whose backing
  storage is a `&'static str` panic-message global.

Observed verbatim:

```
Unsupported construct: Unsupported constant type in translate_constant.

  Rust type : Ty { ... RigidTy(Ref(Region { kind: ReErased },
              Ty { ... RigidTy(Str) }, Not)) }
  pliron type: MirSliceType { element_ty: ... }
  const repr : MirConst { kind: Allocated(Allocation { bytes: [...],
               provenance: ProvenanceMap { ptrs: [(0, Prov(...))] },
               align: 8, mutability: Mut }), ... }

The type dispatch (ZST -> ptr_to_array -> struct -> enum -> float ->
pointer -> integer) did not match this constant. A new handler may
need to be added.
```

So `Str -> MirSliceType<u8>` is sound at the *type* level but the
constant-translation pipeline doesn't know how to materialize a
fat-pointer slice constant.

## Two follow-up directions

### A. Surgical — add a slice/string-constant handler

Add an arm to `translate_constant_value_from_bytes` (and the
top-level `translate_constant` dispatch above it) that, when the
target pliron type is `MirSliceType`, emits an aggregate of:

- the data-pointer constant pointing into the allocation referenced
  by the rust `Allocation` (the first 8 bytes carry the
  provenance-backed offset), and
- the length constant (the next 8 bytes, read as integer).

Concretely, the constant's `Allocation.bytes` already contains both
halves of the fat pointer in the expected slot layout (8B ptr + 8B
len for a 64-bit target). The handler reads them out, builds a
`MirConstantOp` for the integer length, references the pointee
allocation for the data pointer, and wraps both in whatever
aggregate-constant form `MirSliceType` accepts.

Effort: ~half-day if MirSliceType already has a constant-builder
helper; ~1 day if it has to be added.

### B. Structural — DCE before translation on device-fn bodies

The kernel-entry path doesn't crash on the same `DisjointSlice::get_mut`
panic branches, so something upstream prunes them. Likely candidates:

- The kernel-entry path reads `optimized_mir` (which runs the standard
  cleanup MIR opts), the device-fn path reads pre-optimization MIR.
- The kernel-entry path runs an explicit `simplify_branches` /
  `simplify_cfg` pass before translation, the device-fn path doesn't.

If we can identify the asymmetry and route device-fn bodies through
the same pre-translation MIR pipeline, both the original
`RigidTy(Str)` failure and the next-layer
`Unsupported constant type` failure go away together, because the
panic-message `&str` constants are never reached.

Investigation steps:

1. Diff the kernel-entry and device-fn entrypoints into the importer.
   Start at `crates/rustc-codegen-cuda/src/device_codegen.rs:298`
   (`if func.is_kernel { "kernel" } else { "device" }`).
2. Walk each branch through `crates/mir-importer/src/pipeline.rs`
   (the `run_pipeline` / `lower_to_llvm` entrypoints) and identify
   where MIR is fetched and which passes run.
3. If the kernel-entry path uses `optimized_mir` and the device-fn
   path uses raw MIR, switching the device-fn path to
   `optimized_mir` (or running the same subset of passes) is the
   fix. Fallout to expect: any device-fn semantics that relied on
   unoptimized MIR (probably none, but worth a regression sweep
   against `crates/rustc-codegen-cuda/examples/`).

Effort: 1-2 days, plus the regression sweep.

## Recommendation

Land Patch A immediately (verified, isolated, low risk).

For Patch B: keep the current type-translation arm as a stepping
stone, then take direction **B** (structural DCE) as the real fix.
Direction A (slice-constant handler) is a defensible local fix but
treats the symptom -- the panic-message constants exist in MIR only
because the panic branches weren't pruned earlier, and direction B
removes the root cause.

If direction B turns out to be expensive (e.g., the device-fn path
uses raw MIR for a load-bearing reason), fall back to direction A.

## Regression coverage

Add `crates/rustc-codegen-cuda/examples/cross_file_device_fn/` once
direction B lands -- mirror this MWE's bug-b layout (helper in a
sibling file, kernel just calls it). After A+B together, this should
build cleanly. Keep `error_cross_file_device_fn/` capturing the
*pre-fix* state as a regression marker if cuda-oxide's example
harness supports both.
