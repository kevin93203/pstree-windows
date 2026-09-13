use std::env;
use std::ffi::OsString;
use std::process;

#[cfg(windows)]
use std::io::{self, IsTerminal, Write};

#[cfg(windows)]
use pstree_windows::{GlyphSet, ProcessTree, RenderError, collect_processes};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = "Usage: pstree.exe [OPTION]... [PID]\n\nDisplay the Windows process tree.\n\nImplemented options:\n  -A, --ascii                 use ASCII tree-drawing characters\n  -U, --unicode               use Unicode tree-drawing characters\n  -?, --help                  show this help\n  -V, --version               show version information\n\nRecognized for later releases (currently exits with status 2):\n  -h, --highlight-all         highlight the current process and ancestors\n  -p, --show-pids             show process IDs\n  -n, --numeric-sort          sort by PID\n  -c, --compact-not           disable process subtree compaction\n  -s, --show-parents          show parents of the selected process\n  -t, --thread-names          show thread names\n  -T, --hide-threads          hide threads\n\nUnsupported on Windows for now:\n  -a, --arguments             show command-line arguments\n  -l, --long                  do not truncate lines\n  -P, --show-paths             show full executable paths\n  -C, --color=TYPE             color processes by age\n  -H, --highlight-pid=PID      highlight a selected process\n  -g, -k, -N, -S, -u, -Z       Linux-only process metadata options\n\nWithout PID, all top-level process trees are shown. With PID, only that\nprocess and its descendants are shown.\n\nExit status:\n  0  success\n  1  runtime or process lookup failure\n  2  invalid, unsupported, or deferred CLI option\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlyphMode {
    Auto,
    Ascii,
    Unicode,
}

#[derive(Debug, PartialEq, Eq)]
struct CliOptions {
    selected_pid: Option<u32>,
    glyph_mode: GlyphMode,
    show_pids: bool,
    numeric_sort: bool,
    compact: bool,
    show_parents: bool,
    show_threads: bool,
    thread_names: bool,
    highlight_all: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            selected_pid: None,
            glyph_mode: GlyphMode::Auto,
            show_pids: false,
            numeric_sort: false,
            compact: true,
            show_parents: false,
            show_threads: true,
            thread_names: false,
            highlight_all: false,
        }
    }
}

impl CliOptions {
    fn unsupported_options(&self) -> Option<String> {
        let mut options = Vec::new();

        if self.highlight_all {
            options.push("-h/--highlight-all");
        }
        if self.show_pids {
            options.push("-p/--show-pids");
        }
        if self.numeric_sort {
            options.push("-n/--numeric-sort");
        }
        if !self.compact {
            options.push("-c/--compact-not");
        }
        if self.show_parents {
            options.push("-s/--show-parents");
        }
        if self.thread_names {
            options.push("-t/--thread-names");
        }
        if !self.show_threads {
            options.push("-T/--hide-threads");
        }

        (!options.is_empty()).then(|| options.join(", "))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run(CliOptions),
    Help,
    Version,
}

#[derive(Debug, PartialEq, Eq)]
struct CliError(String);

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn main() {
    process::exit(run());
}

fn run() -> i32 {
    let command = match parse_args(env::args_os()) {
        Ok(command) => command,
        Err(error) => return usage_error(error),
    };

    match command {
        Command::Help => {
            print!("{HELP}");
            0
        }
        Command::Version => {
            println!("pstree-windows {VERSION}");
            0
        }
        Command::Run(options) => match options.unsupported_options() {
            Some(options) => usage_error(CliError(format!(
                "option(s) {options} are not supported yet"
            ))),
            None => run_tree(options),
        },
    }
}

fn usage_error(error: CliError) -> i32 {
    eprintln!("pstree: {error}");
    eprintln!("pstree: try '--help' for usage");
    2
}

fn run_tree(options: CliOptions) -> i32 {
    #[cfg(not(windows))]
    {
        let _ = options;
        eprintln!("pstree: this program only supports Windows");
        return 1;
    }

    #[cfg(windows)]
    {
        let processes = match collect_processes() {
            Ok(processes) => processes,
            Err(error) => {
                eprintln!("pstree: failed to enumerate processes: {error}");
                return 1;
            }
        };

        let tree = ProcessTree::from_processes(processes);
        if tree.roots().is_empty() {
            eprintln!("pstree: no processes found");
            return 1;
        }

        let glyph_set = select_glyph_set(options.glyph_mode);
        let output = match tree.render(options.selected_pid, glyph_set) {
            Ok(output) => output,
            Err(RenderError::ProcessNotFound(pid)) => {
                eprintln!("pstree: process {pid} was not found");
                return 1;
            }
        };

        match write_output(&output) {
            Ok(()) => 0,
            Err(error) if is_broken_pipe(&error) => 0,
            Err(error) => {
                eprintln!("pstree: failed to write output: {error}");
                1
            }
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, CliError> {
    let mut args = args.into_iter();
    let _program = args.next();
    let mut options = CliOptions::default();
    let mut positional_only = false;

    for argument in args {
        let argument = argument
            .to_str()
            .ok_or_else(|| CliError("arguments must be valid Unicode".to_owned()))?;

        if !positional_only {
            match argument {
                "-?" | "--help" => return Ok(Command::Help),
                "-V" | "--version" => return Ok(Command::Version),
                "--" => {
                    positional_only = true;
                    continue;
                }
                "--ascii" => set_glyph_mode(&mut options, GlyphMode::Ascii)?,
                "--unicode" => set_glyph_mode(&mut options, GlyphMode::Unicode)?,
                value if value.starts_with("--") => parse_long_option(value, &mut options)?,
                value if value.starts_with('-') => parse_short_options(value, &mut options)?,
                value => set_pid(&mut options, value)?,
            }
        } else {
            set_pid(&mut options, argument)?;
        }
    }

    Ok(Command::Run(options))
}

fn set_glyph_mode(options: &mut CliOptions, mode: GlyphMode) -> Result<(), CliError> {
    if options.glyph_mode != GlyphMode::Auto && options.glyph_mode != mode {
        return Err(CliError(
            "--ascii and --unicode are mutually exclusive".to_owned(),
        ));
    }
    options.glyph_mode = mode;
    Ok(())
}

fn set_pid(options: &mut CliOptions, value: &str) -> Result<(), CliError> {
    if options.selected_pid.is_some() {
        return Err(CliError("only one PID may be specified".to_owned()));
    }
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CliError(format!("invalid PID '{value}'")));
    }

    let pid = value
        .parse::<u32>()
        .map_err(|_| CliError(format!("invalid PID '{value}'")))?;
    if pid == 0 {
        return Err(CliError("PID must be greater than zero".to_owned()));
    }
    options.selected_pid = Some(pid);
    Ok(())
}

fn parse_long_option(value: &str, options: &mut CliOptions) -> Result<(), CliError> {
    match value {
        "--highlight-all" => options.highlight_all = true,
        "--show-pids" => options.show_pids = true,
        "--numeric-sort" => options.numeric_sort = true,
        "--compact-not" => options.compact = false,
        "--show-parents" => options.show_parents = true,
        "--thread-names" => set_thread_names(options)?,
        "--hide-threads" => hide_threads(options)?,
        value if is_unsupported_long_option(value) => {
            return Err(unsupported_option(value));
        }
        value => return Err(CliError(format!("unknown option '{value}'"))),
    }
    Ok(())
}

fn parse_short_options(value: &str, options: &mut CliOptions) -> Result<(), CliError> {
    if value == "-" {
        return Err(CliError("unknown option '-'".to_owned()));
    }
    if value.len() > 1 && value.as_bytes()[1].is_ascii_digit() {
        return Err(CliError(format!("invalid PID '{value}'")));
    }

    for flag in value[1..].chars() {
        match flag {
            'A' => set_glyph_mode(options, GlyphMode::Ascii)?,
            'U' => set_glyph_mode(options, GlyphMode::Unicode)?,
            'h' => options.highlight_all = true,
            'p' => options.show_pids = true,
            'n' => options.numeric_sort = true,
            'c' => options.compact = false,
            's' => options.show_parents = true,
            't' => set_thread_names(options)?,
            'T' => hide_threads(options)?,
            '?' | 'V' => {
                return Err(CliError(format!(
                    "option '-{flag}' must be used on its own"
                )));
            }
            'a' | 'l' | 'P' | 'C' | 'H' | 'g' | 'k' | 'N' | 'S' | 'u' | 'Z' => {
                return Err(unsupported_option(&format!("-{flag}")));
            }
            flag => return Err(CliError(format!("unknown option '-{flag}'"))),
        }
    }
    Ok(())
}

fn set_thread_names(options: &mut CliOptions) -> Result<(), CliError> {
    if !options.show_threads {
        return Err(CliError(
            "--thread-names and --hide-threads are mutually exclusive".to_owned(),
        ));
    }
    options.thread_names = true;
    Ok(())
}

fn hide_threads(options: &mut CliOptions) -> Result<(), CliError> {
    if options.thread_names {
        return Err(CliError(
            "--thread-names and --hide-threads are mutually exclusive".to_owned(),
        ));
    }
    options.show_threads = false;
    Ok(())
}

fn is_unsupported_long_option(value: &str) -> bool {
    let name = value.split_once('=').map_or(value, |(name, _)| name);
    matches!(
        name,
        "--arguments"
            | "--long"
            | "--show-paths"
            | "--color"
            | "--highlight-pid"
            | "--show-pgids"
            | "--kthreads"
            | "--ns-sort"
            | "--ns-changes"
            | "--uid-changes"
            | "--security-context"
    )
}

fn unsupported_option(value: &str) -> CliError {
    CliError(format!("option '{value}' is not supported yet"))
}

#[cfg(windows)]
fn select_glyph_set(mode: GlyphMode) -> GlyphSet {
    match mode {
        GlyphMode::Ascii => GlyphSet::Ascii,
        GlyphMode::Unicode => GlyphSet::Unicode,
        GlyphMode::Auto => {
            if io::stdout().is_terminal() {
                GlyphSet::Unicode
            } else {
                GlyphSet::Ascii
            }
        }
    }
}

#[cfg(windows)]
fn write_output(output: &str) -> io::Result<()> {
    if !io::stdout().is_terminal() {
        return io::stdout().write_all(output.as_bytes());
    }

    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{CONSOLE_MODE, GetConsoleMode, WriteConsoleW};

    let handle = HANDLE(io::stdout().as_raw_handle());
    let mut mode = CONSOLE_MODE(0);
    if unsafe { GetConsoleMode(handle, &mut mode) }.is_err() {
        return io::stdout().write_all(output.as_bytes());
    }

    let wide = output.encode_utf16().collect::<Vec<_>>();
    let mut written: u32 = 0;
    unsafe { WriteConsoleW(handle, &wide, Some(&mut written as *mut u32), None) }
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(windows)]
fn is_broken_pipe(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os_args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_default_options() {
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "123"])),
            Ok(Command::Run(CliOptions {
                selected_pid: Some(123),
                glyph_mode: GlyphMode::Auto,
                show_pids: false,
                numeric_sort: false,
                compact: true,
                show_parents: false,
                show_threads: true,
                thread_names: false,
                highlight_all: false,
            }))
        );
    }

    #[test]
    fn parses_glyph_overrides() {
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "--ascii"])),
            Ok(Command::Run(CliOptions {
                selected_pid: None,
                glyph_mode: GlyphMode::Ascii,
                ..CliOptions::default()
            }))
        );
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "--unicode", "123"])),
            Ok(Command::Run(CliOptions {
                selected_pid: Some(123),
                glyph_mode: GlyphMode::Unicode,
                ..CliOptions::default()
            }))
        );
    }

    #[test]
    fn parses_short_aliases_and_bundles() {
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "-A"])),
            parse_args(os_args(&["pstree.exe", "--ascii",]))
        );
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "-U"])),
            parse_args(os_args(&["pstree.exe", "--unicode",]))
        );

        let Command::Run(options) = parse_args(os_args(&["pstree.exe", "-pncst"])).unwrap() else {
            panic!("expected a run command");
        };
        assert!(options.show_pids);
        assert!(options.numeric_sort);
        assert!(!options.compact);
        assert!(options.show_parents);
        assert!(options.thread_names);
        assert_eq!(
            options.unsupported_options().as_deref(),
            Some(
                "-p/--show-pids, -n/--numeric-sort, -c/--compact-not, -s/--show-parents, -t/--thread-names"
            )
        );

        for option in [
            "--highlight-all",
            "--show-pids",
            "--numeric-sort",
            "--compact-not",
            "--show-parents",
            "--thread-names",
            "--hide-threads",
        ] {
            let Command::Run(options) = parse_args(os_args(&["pstree.exe", option])).unwrap()
            else {
                panic!("expected a run command");
            };
            assert!(options.unsupported_options().is_some(), "option: {option}");
        }
    }

    #[test]
    fn parses_double_dash_and_rejects_second_pid() {
        let Command::Run(options) = parse_args(os_args(&["pstree.exe", "--", "123"])).unwrap()
        else {
            panic!("expected a run command");
        };
        assert_eq!(options.selected_pid, Some(123));
        assert!(parse_args(os_args(&["pstree.exe", "--", "123", "456"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "--", "-1"])).is_err());
    }

    #[test]
    fn parses_help_and_version_aliases() {
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "-?"])),
            Ok(Command::Help)
        );
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "--help", "--wat"])),
            Ok(Command::Help)
        );
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "-V"])),
            Ok(Command::Version)
        );
    }

    #[test]
    fn rejects_conflicting_glyphs() {
        assert!(parse_args(os_args(&["pstree.exe", "-A", "-U"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "-tT"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "-T", "-t"])).is_err());
    }

    #[test]
    fn rejects_invalid_pid_and_unknown_options() {
        assert!(parse_args(os_args(&["pstree.exe", "0"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "abc"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "-1"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "4294967296"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "-"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "--wat"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "1", "2"])).is_err());
    }

    #[test]
    fn recognizes_deferred_and_rejects_unsupported_options() {
        let Command::Run(options) =
            parse_args(os_args(&["pstree.exe", "--highlight-all"])).unwrap()
        else {
            panic!("expected a run command");
        };
        assert_eq!(
            options.unsupported_options().as_deref(),
            Some("-h/--highlight-all")
        );

        for option in [
            "-a",
            "--arguments",
            "-P",
            "--color=age",
            "-H",
            "-g",
            "--ns-sort=pid",
        ] {
            let error = parse_args(os_args(&["pstree.exe", option])).unwrap_err();
            assert!(error.to_string().contains("not supported yet"));
        }
    }
}
