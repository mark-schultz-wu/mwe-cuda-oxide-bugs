# mwe-cuda-oxide-bugs

Two minimal compile-time reproducers for cuda-oxide bugs, plus draft
patches against [`NVlabs/cuda-oxide`](https://github.com/NVlabs/cuda-oxide)
`main`.

Pinned cuda-oxide SHA: **c8b3103cb1ffe1664aa1458b467ebc2206d5d82a**
(toolchain `nightly-2026-04-03`). Both reproducers and patches were
verified end-to-end on a CUDA 12.6 host -- see `WRITEUP.md` for the
full procedure and observed outputs.

## Layout

```
mwe-cuda-oxide-bugs/
├── Cargo.toml               # umbrella; excludes the bug crates
├── rust-toolchain.toml      # matches cuda-oxide main's pin exactly
├── bug-a-tuple-return/      # standalone crate, own workspace
│   └── src/main.rs          # ~50 LoC, USE_TUPLE bool toggles bug
├── bug-b-cross-file/        # standalone crate, own workspace
│   └── src/
│       ├── main.rs          # INLINE_HELPER bool toggles bug
│       └── helpers.rs       # sibling file with the cross-file helper
└── patches/
    ├── patch-a-tuple-return.diff   # 8-line targeted fix (verified)
    ├── patch-b-cross-file.diff     # type-arm probe (advances failure
    │                                # to next layer; see PLAN)
    └── patch-b-cross-file-PLAN.md  # residual risk + structural plan
```

Each bug crate is a separate workspace so it can carry its own
toolchain if needed; both currently inherit the top-level
`rust-toolchain.toml`.

## Bug A: `(T, U)` device-helper return → pliron erase-with-use panic

**Repro:**

```bash
cd bug-a-tuple-return
cargo oxide build --arch sm_89
```

**Observed error (verbatim):**

```
thread 'rustc' (...) panicked at .../pliron/src/operation.rs:526:9:
Operation with use(s) being erased
```

Backtrace ends in `mir_lower::convert::ops::call::convert` at
`crates/mir-lower/src/convert/ops/call.rs:411` (the
`rewriter.erase_operation(ctx, op);` line).

**Source-level toggle that makes it go away:** flip `USE_TUPLE` to
`false` in `bug-a-tuple-return/src/main.rs`. The build then uses
`split_u128_array` (returns `[u64; 2]`) and succeeds.

**Bug location in cuda-oxide:**
`crates/mir-lower/src/convert/ops/call.rs:369-378` --
variable named `is_unit` but check is `is::<MirTupleType>()`, matching
*any* tuple. Patched by `patches/patch-a-tuple-return.diff` -- verified
to clear the panic and produce a successful PTX build.

## Bug B: cross-file device-helper → `RigidTy(Str)`

**Repro:**

```bash
cd bug-b-cross-file
cargo oxide build --arch sm_89
```

**Observed error (verbatim):**

```
error: [rustc_codegen_cuda] Device codegen failed: PTX generation failed:
Translation failed: bug_b: Compilation error: invalid input program.
Unsupported construct: Type translation not yet implemented for: RigidTy(Str)
```

Concrete error site:
`crates/mir-importer/src/translator/types.rs:705-708` -- the catch-all
`Type translation not yet implemented for: {:?}` arm.

**Source-level toggle that makes it go away:** set `INLINE_HELPER` to
`true` in `bug-b-cross-file/src/main.rs`. The same body inlined into
the kernel takes the kernel-entry lowering path and produces valid PTX.

**Patch status:** `patches/patch-b-cross-file.diff` adds an explicit
`Str → MirSliceType<u8>` arm (layout-identical) as a candidate fix.
**Runtime verification result**: the patch successfully clears
the original `Type translation not yet implemented` error but exposes
a downstream failure in `translate_constant`
(`crates/mir-importer/src/translator/rvalue.rs:2315`) which has no
handler for fat-pointer slice constants. See
`patches/patch-b-cross-file-PLAN.md` for the full failure mode and
the recommended structural follow-up.

## Applying the patches

```bash
git clone https://github.com/NVlabs/cuda-oxide
cd cuda-oxide
git checkout c8b3103
git apply /path/to/mwe-cuda-oxide-bugs/patches/patch-a-tuple-return.diff
git apply /path/to/mwe-cuda-oxide-bugs/patches/patch-b-cross-file.diff
```

To produce a patched `librustc_codegen_cuda.so` from a source build:

```bash
RUSTFLAGS="-L native=$(rustc --print sysroot)/lib" \
  cargo build --release --manifest-path crates/rustc-codegen-cuda/Cargo.toml
# Then either: replace ~/.cargo/cuda-oxide/librustc_codegen_cuda.so with
# crates/rustc-codegen-cuda/target/release/librustc_codegen_cuda.so,
# or override `-Z codegen-backend=` at the rustc invocation site.
# Also: set CUDA_OXIDE_LLC=$(rustc --print sysroot)/lib/rustlib/$(rustc -vV \
#   | sed -n 's|host: ||p')/bin/llc if you need an llvm-22 llc on PATH.
```
