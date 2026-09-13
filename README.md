# pstree-windows

A small Windows process-tree viewer written in Rust.

## Usage

```text
pstree.exe [--ascii | --unicode] [PID]
```

Without a PID, the program prints every top-level process and its descendants.
With a PID, it prints only that process and its descendants. Each process is
shown as `name(PID)` and child processes are sorted by PID.

Interactive Windows consoles use Unicode tree characters by default. Redirected
output uses ASCII characters by default. Use `--ascii` or `--unicode` to force
a style.

The v1 collector uses the Win32 Toolhelp32 process snapshot API and does not
require administrator privileges. It intentionally does not show command lines,
paths, users, threads, or live updates.

## Build

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

The executable is written to
`target\x86_64-pc-windows-msvc\release\pstree.exe`.
