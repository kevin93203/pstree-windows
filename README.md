# pstree-windows

A small Windows process-tree viewer written in Rust.

## Install

On Windows x64, install the latest release with PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/kevin93203/pstree-windows/releases/latest/download/install.ps1 | iex"
```

The installer downloads the verified release archive, installs `pstree.exe`
to `%LOCALAPPDATA%\pstree\bin`, and adds that directory to the current user's
`PATH`. Restart the terminal after installation, then run:

```powershell
pstree --version
```

To pin a release, set `PSTREE_VERSION` in the same PowerShell process:

```powershell
powershell -ExecutionPolicy Bypass -c "$env:PSTREE_VERSION='v0.1.0'; irm https://github.com/kevin93203/pstree-windows/releases/latest/download/install.ps1 | iex"
```

The script can also be downloaded and inspected before execution:

```powershell
irm https://github.com/kevin93203/pstree-windows/releases/latest/download/install.ps1 -OutFile install.ps1
.\install.ps1
```

To publish a release, update the version in `Cargo.toml`, commit the change,
then push a matching tag. GitHub Actions builds and publishes the release
artifacts automatically:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

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
-p, --show-pids   show process and thread IDs
-n, --numeric-sort
                   sort processes by PID
-c, --compact-not disable process and thread compaction
-s, --show-parents
                   show parents of a selected PID
-t, --thread-names query full thread names
-T, --hide-threads
                   hide thread pseudo-children
```

The options `-p`, `-n`, `-c`, `-s`, `-t`, and `-T` are implemented for the
Windows process/thread tree. `-s` requires a PID. By default, identical process
subtrees and adjacent named threads are compacted. `-c` disables both forms of
compaction. `-p` also disables both forms so every process and thread ID remains
distinguishable. Use `--` before a PID when needed.

`-h` and other Linux-only metadata options are recognized or rejected with
exit code `2`; no unsupported option is silently ignored.

Without a PID, the program prints every top-level process and its descendants.
With a PID, it prints only that process and its descendants. Process IDs are
hidden by default and enabled with `-p`. Processes are sorted by name by
default, or by PID with `-n`.

The tree follows Linux `pstree`'s horizontal layout: the parent and first child
share a line, while later siblings are printed on aligned lines. For example:

```text
root-+-child-a
     |-child-b
     `-child-c
```

Unicode output uses the equivalent `─┬─`, `├─`, and `└─` branch characters.

Threads are shown by default and use the owning process name in braces, so
identical thread names are compacted like Linux `pstree`. `-t` queries full
Windows thread descriptions; failed or unavailable descriptions fall back to
the owning process name. With `-p`, named threads use `{Name}(n)`, and
thread grouping is disabled so each TID remains visible.
Threads and process children share one sibling list: default sorting compares
their displayed names, while `-n` compares PID/TID numerically.

Interactive Windows consoles use Unicode tree characters by default. Redirected
output uses ASCII characters by default. Use `--ascii` or `--unicode` to force
a style.

The collector uses one Win32 Toolhelp32 snapshot for processes and threads and
does not require administrator privileges. Thread descriptions are best effort;
permission failures or threads that exit during collection fall back to the
owning process name. Use `-p` when the TID must remain visible.
The tool does not show command lines, paths, users, or live updates.

Exit codes are `0` for help, version, or successful output, `1` for runtime or
process lookup failures, and `2` for invalid, unsupported, or deferred options.

## Build

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

The executable is written to
`target\x86_64-pc-windows-msvc\release\pstree.exe`.
