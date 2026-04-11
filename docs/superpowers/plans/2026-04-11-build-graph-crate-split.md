# CBXShell Build Graph Crate Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce rebuild fan-out and late serialized work by splitting `cbxshell` into narrower crates while preserving all runtime archive and image functionality.

**Architecture:** Keep the shipping DLL crate as `cbxshell`, but move reusable logic into `cbxshell-core`, `cbxshell-archive`, `cbxshell-image`, and `cbxshell-gui-win`. Make `cbxmanager` a thin binary package so GUI edits stop recompiling the DLL path, and trim `native-windows-gui` features to only the controls proven by code search.

**Tech Stack:** Rust workspace crates, Cargo features as build boundaries, `windows`, `native-windows-gui`, `image`, `zip`, `unrar`, `sevenz-rust`

---

## File Map

- Create: `crates/cbxshell-core/Cargo.toml`
- Create: `crates/cbxshell-core/src/lib.rs`
- Create: `crates/cbxshell-archive/Cargo.toml`
- Create: `crates/cbxshell-archive/src/lib.rs`
- Create: `crates/cbxshell-image/Cargo.toml`
- Create: `crates/cbxshell-image/src/lib.rs`
- Create: `crates/cbxshell-gui-win/Cargo.toml`
- Create: `crates/cbxshell-gui-win/src/lib.rs`
- Create: `cbxmanager/Cargo.toml`
- Create: `cbxmanager/src/main.rs`
- Modify: `Cargo.toml`
- Modify: `CBXShell/Cargo.toml`
- Modify: `CBXShell/build.rs`
- Modify: `CBXShell/src/lib.rs`
- Modify: `CBXShell/src/com/*.rs`
- Modify: `CBXShell/src/registry.rs`
- Modify: `CBXShell/src/manager/*.rs` or move those modules into `crates/cbxshell-gui-win/src/`
- Modify: `CBXShell/tests/test_webp_decode.rs`

## Dependency Direction

- `cbxshell-core`: shared error/logging/domain helpers; no GUI or `windows` dependency
- `cbxshell-archive`: archive open/list/extract + stream reader; depends on `cbxshell-core`
- `cbxshell-image`: image decode/resize/thumbnail helpers; depends on `cbxshell-core`
- `cbxshell-gui-win`: manager state + registry operations + NWG UI; depends on `cbxshell-core`
- `cbxshell` DLL crate: COM entrypoints + registry registration + Windows interop; depends on `cbxshell-core`, `cbxshell-archive`, `cbxshell-image`
- `cbxmanager` binary: thin `main`, depends on `cbxshell-gui-win`

## Verification Matrix

- Clean baseline: `cargo build --release --timings`
- Clean baseline: `cargo build`
- Clean baseline: `cargo check`
- Dirty baseline: touch a core/logic file, run `cargo build`
- Dirty baseline: touch a GUI file, run `cargo build`
- Repeat the same matrix after the refactor in a fresh target directory
- Confirm `native-windows-gui` still builds with explicit features only

### Task 1: Capture the baseline in a writable Cargo home

**Files:**
- Modify: none

- [ ] Create a local Cargo home under the workspace so baseline commands do not fail on the global registry cache permissions.
- [ ] Run `cargo build --release --timings`, `cargo build`, and `cargo check` with that local Cargo home and record wall-clock results.
- [ ] Touch one archive/image-facing source file and one manager UI source file, then rerun `cargo build` after each touch to capture dirty rebuild behavior.

### Task 2: Introduce pure shared core boundaries

**Files:**
- Create: `crates/cbxshell-core/Cargo.toml`
- Create: `crates/cbxshell-core/src/lib.rs`
- Modify: `CBXShell/src/utils/error.rs`
- Modify: `CBXShell/src/utils/debug_log.rs`

- [ ] Move `CbxError`, the shared `Result` alias, and debug logging into `cbxshell-core`.
- [ ] Keep `cbxshell-core` free of `windows` and GUI dependencies by removing `HRESULT` conversion from the shared crate.
- [ ] Add focused tests in `cbxshell-core` for error/debug-log behavior that already existed in the monolith.

### Task 3: Split archive and image crates without dropping formats

**Files:**
- Create: `crates/cbxshell-archive/Cargo.toml`
- Create: `crates/cbxshell-archive/src/lib.rs`
- Create: `crates/cbxshell-image/Cargo.toml`
- Create: `crates/cbxshell-image/src/lib.rs`
- Modify: `CBXShell/src/archive/*.rs`
- Modify: `CBXShell/src/image_processor/*.rs`

- [ ] Move archive modules into `cbxshell-archive` with no dependency on GUI code.
- [ ] Move image modules into `cbxshell-image` with no dependency on archive code except via data passed from callers.
- [ ] Replace archive-to-image magic verification reach-through with an explicit dependency edge from `cbxshell` or a small `cbxshell-image` API consumed by `cbxshell-archive`.
- [ ] Keep every existing archive format and image decoder enabled in Cargo manifests.

### Task 4: Keep the DLL crate thin and Windows-specific

**Files:**
- Modify: `CBXShell/Cargo.toml`
- Modify: `CBXShell/src/lib.rs`
- Modify: `CBXShell/src/com/*.rs`
- Modify: `CBXShell/src/registry.rs`

- [ ] Reduce `cbxshell` to COM entrypoints, registry registration, and Windows glue.
- [ ] Re-export the existing public surface that callers still expect where reasonable.
- [ ] Add local `CbxError -> HRESULT` conversion in the DLL crate so `cbxshell-core` stays platform-agnostic.

### Task 5: Make `cbxmanager` a thin package over `cbxshell-gui-win`

**Files:**
- Create: `crates/cbxshell-gui-win/Cargo.toml`
- Create: `crates/cbxshell-gui-win/src/lib.rs`
- Create: `cbxmanager/Cargo.toml`
- Create: `cbxmanager/src/main.rs`
- Modify: `CBXShell/src/manager/*.rs`

- [ ] Move manager state, registry operations, and UI code into `cbxshell-gui-win`.
- [ ] Replace the in-package bin target with a new `cbxmanager` package whose `main.rs` only initializes NWG and launches the UI.
- [ ] Preserve registry behavior and DLL registration/unregistration behavior without forcing the manager to depend on archive/image heavy code.

### Task 6: Minimize `native-windows-gui` features by proof

**Files:**
- Modify: `crates/cbxshell-gui-win/Cargo.toml`

- [ ] Replace the default `native-windows-gui` feature set with `default-features = false`.
- [ ] Keep only features proven by code usage and crate feature gates.
- [ ] Record the removed features and the source references showing the app only uses `Window`, `Menu`, `MenuItem`, `MenuSeparator`, `Font`, `Label`, `Frame`, `CheckBox`, and `Button`.

### Task 7: Re-run the measurement matrix and compare fan-out

**Files:**
- Modify: none

- [ ] Run the same clean and dirty build commands after the split in a fresh target directory.
- [ ] Compare baseline vs. refactor results, with focus on dirty rebuild improvements and whether GUI edits stop forcing the DLL-heavy path.
- [ ] Summarize remaining risks, especially any parts that still serialize through the DLL crate.
