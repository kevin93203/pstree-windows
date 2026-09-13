# pstree-windows

A small Windows process-tree viewer written in Rust.

## Usage

```text
pstree.exe [OPTION]... [PID]
```

Implemented options:

```text
-A, --ascii       use ASCII tree-drawing characters
-U, --unicode     use Unicode tree-drawing characters
-?, --help        show help
-V, --version     show version information
```

The Linux-compatible options `-h`, `-p`, `-n`, `-c`, `-s`, `-t`, and `-T`
are recognized as part of the CLI contract but currently return exit code `2`
with an explicit not-supported message. Other Linux-only options are rejected
the same way; no unsupported option is silently ignored. Use `--` before a
PID when needed.

Without a PID, the program prints every top-level process and its descendants.
With a PID, it prints only that process and its descendants. Each process is
shown as `name(PID)` and child processes are sorted by PID.

Interactive Windows consoles use Unicode tree characters by default. Redirected
output uses ASCII characters by default. Use `--ascii` or `--unicode` to force
a style.

P0 establishes the CLI contract only. The current renderer still uses the v1
output format; PID display, name sorting, thread output, and subtree compaction
will be implemented in later phases.

The v1 collector uses the Win32 Toolhelp32 process snapshot API and does not
require administrator privileges. It intentionally does not show command lines,
paths, users, threads, or live updates.

Exit codes are `0` for help, version, or successful output, `1` for runtime or
process lookup failures, and `2` for invalid, unsupported, or deferred options.

## Build

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

The executable is written to
`target\x86_64-pc-windows-msvc\release\pstree.exe`.
