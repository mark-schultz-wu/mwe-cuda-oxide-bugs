# mwe-cuda-oxide-bugs

Two minimal compile-time reproducers for cuda-oxide bugs, plus draft
patches against [`NVlabs/cuda-oxide`](https://github.com/NVlabs/cuda-oxide)
`main`.

Pinned cuda-oxide SHA: **c8b3103cb1ffe1664aa1458b467ebc2206d5d82a**
(toolchain `nightly-2026-04-03`).

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
    ├── patch-a-tuple-return.diff   # 8-line targeted fix
    ├── patch-b-cross-file.diff     # candidate type-arm fix
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

**Expected error (verbatim):**

```
thread 'rustc' panicked at pliron/src/operation.rs:526:9:
Operation with use(s) being erased
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

Backtrace ends in `mir_lower::convert::ops::call::convert`.

**Source-level toggle that makes it go away:** flip `USE_TUPLE` to
`false` in `bug-a-tuple-return/src/main.rs`. The build then uses
`split_u128_array` (returns `[u64; 2]`) and succeeds.

**Bug location in cuda-oxide:**
`crates/mir-lower/src/convert/ops/call.rs:369-378` —
variable named `is_unit` but check is `is::<MirTupleType>()`, matching
*any* tuple. Patched by `patches/patch-a-tuple-return.diff`.

## Bug B: cross-file `#[inline(always)]` device helper → `RigidTy(Str)`

**Repro:**

```bash
cd bug-b-cross-file
cargo oxide build --arch sm_89
```

**Expected error (verbatim, abridged):**

```
[rustc_codegen_cuda] Device codegen failed: PTX generation failed:
Translation failed: foo: Compilation error: invalid input program.
Unsupported construct: Type translation not yet implemented for: RigidTy(Str)
```

(Concrete error site:
`crates/mir-importer/src/translator/types.rs:705-708` — the catch-all
"Type translation not yet implemented for: {:?}" arm.)

**Source-level toggle that makes it go away:** set `INLINE_HELPER` to
`true` in `bug-b-cross-file/src/main.rs`. The same body inlined into
the kernel takes the kernel-entry lowering path and produces valid PTX.

**Bug location in cuda-oxide:** the divergence between
kernel-entry and device-function lowering paths. The kernel-entry path
either DCEs the `panic_bounds_check`'s `&str` arg before type
translation or uses a different translator entry; the device-fn path
hits `translate_type`'s `_` arm on bare `RigidTy::Str`.

**Patch status:** `patches/patch-b-cross-file.diff` adds an explicit
`Str → MirSliceType<u8>` arm (layout-identical) as a probe / candidate
fix. See `patches/patch-b-cross-file-PLAN.md` for residual risk and
the structural follow-up.

## Local-only caveat

Verification (running `cargo oxide build` to observe the failures and
confirm the patches eliminate them) requires:

1. cuda-oxide's `cargo-oxide` cargo subcommand installed
   (`cargo install --path crates/cargo-oxide` from a cuda-oxide
   checkout).
2. A working CUDA toolchain (nvcc, libnvvm) discoverable to
   `rustc-codegen-cuda`.

Neither was set up on the machine that produced this MWE. The patches
were verified to apply cleanly (`git apply --check`) against
cuda-oxide `main@c8b3103`; runtime verification is left to the
reviewer.

## Applying the patches

```bash
git clone https://github.com/NVlabs/cuda-oxide
cd cuda-oxide
git checkout c8b3103
git apply /path/to/mwe-cuda-oxide-bugs/patches/patch-a-tuple-return.diff
git apply /path/to/mwe-cuda-oxide-bugs/patches/patch-b-cross-file.diff
```
