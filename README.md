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
