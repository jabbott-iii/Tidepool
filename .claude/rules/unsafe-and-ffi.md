---
paths:
  - "crates/sys/**"
  - "crates/platform/**"
---

# Rules for `unsafe` and FFI code

These two crates are the only ones allowed to contain `unsafe`.

- **Declarations belong in `tidepool-sys`.** `platform` calls them. Never
  write `extern` blocks in `platform`.
- **To add bindings, follow `/add-ffi-binding`.** Declare only what you
  call; cite the header or doc for each item; assert `size_of`/`align_of`
  (and `offset_of!` for fields we read) with values from the header.
- **Edition 2024 syntax:** `unsafe extern "system" { ... }` for Win32,
  `unsafe extern "C" { ... }` elsewhere.
- **One unsafe operation per `unsafe` block,** each preceded by
  `// SAFETY:` explaining pointer validity, lifetimes, thread affinity and
  initialization.
- **Every `pub unsafe fn`** has a `# Safety` section.
- **Objective-C:** always `msg_send!` with the exact signature; `_stret` for
  struct returns over 16 bytes on x86_64; an autorelease pool per loop turn;
  AppKit on the main thread only.
- **Ownership:** document who frees every pointer an OS call returns, and
  how (`XFree`, `CFRelease`, `release`, `CloseHandle`…). Wrap it in RAII.
- **Errors:** convert `errno`, `GetLastError`/`HRESULT` and `NSError` into
  our error enums with context. Never ignore a return code without a comment
  saying why that is correct.
- **Signals:** handlers only write a byte to a self-pipe.
- **Processes and PTYs:** spawn with argv vectors; close inherited fds
  (`O_CLOEXEC`); reap every child.
- **Terminal state:** the raw-mode guard restores the terminal on drop,
  panic and signals.
- **Before reporting done,** run `cargo xtask unsafe-audit` (once it
  exists) and ask `security-reviewer` to review. If layout assertions can
  only compile on another OS, say which OS must verify them.
