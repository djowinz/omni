# Idiomatic Rust Simplification

Comprehensive cleanup of the omni codebase for idiomatic Rust patterns, DRY compliance, consistent error handling, and proper unsafe documentation.

## Approach

By-crate, prioritized: `omni-shared` first (small, foundational), then `omni-host` (biggest payoff), then `omni-overlay-dll` (mostly unavoidable unsafe FFI). Within each crate, all categories are applied per-file in a single pass.

## Categories

### 1. Unsafe Code Documentation

Every `unsafe` block gets a `// SAFETY:` comment explaining why it is sound. Currently ~115 blocks exist across all three crates, many undocumented. The comment must state the invariants relied upon (valid handle, single-thread access, layout compatibility, etc.).

DLL globals (`static mut`) get block-level comments explaining the concurrency model: single render thread, hooks disabled before cleanup, `SeqCst` ordering on guard flags.

### 2. Toolhelp32 Snapshot Abstraction (DRY)

The Toolhelp32 iterate-modules pattern is repeated 6+ times across `scanner.rs` and `injector/mod.rs`. Each instance follows the same skeleton: create snapshot, init entry with `dwSize`, call `First`, loop `Next`, call `CloseHandle` at every exit point.

Two simple abstractions eliminate this duplication:

**`OwnedHandle`** -- a thin RAII wrapper that calls `CloseHandle` on `Drop`:

```rust
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: handle was obtained from a Win32 API that returns
        // a valid handle on success. We own it exclusively.
        unsafe { let _ = CloseHandle(self.0); }
    }
}
```

This eliminates ~15 manual `CloseHandle` calls at different exit points.

**`iter_modules(pid) -> Result<impl Iterator<Item = MODULEENTRY32W>>`** -- encapsulates the snapshot + First/Next loop, yielding entries. A similar `iter_processes()` helper handles process enumeration.

With these in place, the following functions collapse to simple iterator operations:
- `has_module()` -- `iter_modules(pid)?.any(|m| name matches)`
- `has_graphics_dll()` -- `iter_modules(pid)?.any(|m| name in GRAPHICS_DLLS)`
- `get_process_exe_path()` -- `iter_modules(pid)?.next()` then read path
- `is_system_process()` -- `iter_modules(pid)?.next()` then check path prefix
- `find_remote_module()` -- `iter_modules(pid)?.find(|m| name matches)` then read base addr
- `find_remote_module_path()` -- `iter_modules(pid)?.find(|m| name matches)` then read path

The injector stops reaching into `crate::scanner::wchar_to_string` because everything routes through shared helpers.

### 3. Error Handling Standardization

**Host crate** -- a `HostError` enum covering the major failure domains:

```rust
pub enum HostError {
    Win32(windows::core::Error),
    Io(std::io::Error),
    Message(String),
}
```

With `From` impls for `windows::core::Error`, `std::io::Error`, and `String`. Implements `std::fmt::Display` and `std::error::Error`. Replaces all `Box<dyn Error>` and `Result<T, String>` returns in the host.

**DLL crate** -- stays with `String` errors. The DLL only logs errors to file (never propagates them across FFI boundaries), so a richer type adds no value. All fallible DLL functions consistently return `Result<T, String>`.

**Shared crate** -- no error types needed (infallible data types).

### 4. Code Organization & Idiomatic Patterns

**Import ordering** -- standardize all files to three groups separated by blank lines:

```rust
use std::...;           // stdlib

use tracing::...;       // external crates

use crate::...;         // local modules
```

**`LazyLock` modernization** -- replace `Mutex<Option<HashSet<T>>>` lazy-init patterns with `std::sync::LazyLock` (stable since Rust 1.80):

```rust
// Before
static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

// After
static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
```

**`RemoteAlloc` RAII guard** -- wrap `VirtualAllocEx`/`VirtualFreeEx` in a guard so cleanup is automatic even on early returns, eliminating manual `VirtualFreeEx` at 3 different error paths in `do_injection()`:

```rust
struct RemoteAlloc {
    process: HANDLE,
    ptr: *mut c_void,
}

impl Drop for RemoteAlloc {
    fn drop(&mut self) {
        // SAFETY: process handle is valid for the lifetime of the injection
        // operation. ptr was returned by VirtualAllocEx on this process.
        unsafe { let _ = VirtualFreeEx(self.process, self.ptr, 0, MEM_RELEASE); }
    }
}
```

**`write_fixed_str` tail zeroing** -- replace the byte-by-byte loop with `dest[copy_len..].fill(0)`.

## Phase Plan

### Phase 1: `omni-shared` (4 files, ~550 LOC)

- `// SAFETY:` comments on atomic operations in `ipc_protocol.rs`
- `fill(0)` cleanup in `write_fixed_str` (`widget_types.rs`)
- Import ordering across all files

### Phase 2: `omni-host` (31 files, ~7,500 LOC)

- New `error.rs` module with `HostError` enum
- New `win32.rs` module with `OwnedHandle`, `iter_modules()`, `iter_processes()`, `wchar_to_string()`
- Refactor `scanner.rs` to use `win32::` helpers (collapse 5 functions)
- Refactor `injector/mod.rs` to use `win32::` helpers + `RemoteAlloc` guard (collapse 3 functions, eliminate manual cleanup)
- Convert all `Box<dyn Error>` and `String` error returns to `HostError`
- `LazyLock` modernization where applicable
- `// SAFETY:` comments on all ~90 host-side unsafe blocks
- Import ordering across all 31 files

### Phase 3: `omni-overlay-dll` (7 files, ~2,100 LOC)

- `// SAFETY:` comments on all ~55 DLL-side unsafe blocks
- Consistent `Result<T, String>` returns
- Import ordering across all 7 files

## Out of Scope (YAGNI)

- No external error crate dependency (`thiserror`, `anyhow`)
- No restructuring of module hierarchy (already clean)
- No touching the DLL's `static mut` globals (necessary for hook callbacks, already well-guarded with `AtomicBool` + `SeqCst`)
- No new abstractions beyond the three small ones (`OwnedHandle`, `RemoteAlloc`, `iter_modules`/`iter_processes`)
- No adding tests, docstrings, or type annotations to unchanged code
