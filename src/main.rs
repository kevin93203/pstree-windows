use std::env;
use std::ffi::OsString;
use std::process;

#[cfg(windows)]
use std::io::{self, IsTerminal, Write};

#[cfg(windows)]
use pstree_windows::{GlyphSet, ProcessTree, RenderError, collect_processes};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = "Usage: pstree.exe [--ascii | --unicode] [PID]\n\nDisplay the Windows process tree.\n\nOptions:\n  --ascii       use ASCII tree-drawing characters\n  --unicode     use Unicode tree-drawing characters\n  -h, --help    show this help\n  -V, --version show version information\n\nWithout PID, all top-level process trees are shown. With PID, only that\nprocess and its descendants are shown.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlyphMode {
    Auto,
    Ascii,
    Unicode,
}

#[derive(Debug, PartialEq, Eq)]
struct Options {
    pid: Option<u32>,
    glyph_mode: GlyphMode,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run(Options),
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
        Err(error) => {
            eprintln!("pstree: {error}");
            eprintln!("pstree: try '--help' for usage");
            return 2;
        }
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
        Command::Run(options) => run_tree(options),
    }
}

fn run_tree(options: Options) -> i32 {
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
        let output = match tree.render(options.pid, glyph_set) {
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
    let mut pid = None;
    let mut glyph_mode = GlyphMode::Auto;

    for argument in args {
        let argument = argument
            .to_str()
            .ok_or_else(|| CliError("arguments must be valid Unicode".to_owned()))?;

        match argument {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--ascii" => {
                if glyph_mode == GlyphMode::Unicode {
                    return Err(CliError(
                        "--ascii and --unicode are mutually exclusive".to_owned(),
                    ));
                }
                glyph_mode = GlyphMode::Ascii;
            }
            "--unicode" => {
                if glyph_mode == GlyphMode::Ascii {
                    return Err(CliError(
                        "--ascii and --unicode are mutually exclusive".to_owned(),
                    ));
                }
                glyph_mode = GlyphMode::Unicode;
            }
            value if value.starts_with('-') => {
                return Err(CliError(format!("unknown option '{value}'")));
            }
            value => {
                if pid.is_some() {
                    return Err(CliError("only one PID may be specified".to_owned()));
                }
                let parsed = value
                    .parse::<u32>()
                    .map_err(|_| CliError(format!("invalid PID '{value}'")))?;
                if parsed == 0 {
                    return Err(CliError("PID must be greater than zero".to_owned()));
                }
                pid = Some(parsed);
            }
        }
    }

    Ok(Command::Run(Options { pid, glyph_mode }))
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
            Ok(Command::Run(Options {
                pid: Some(123),
                glyph_mode: GlyphMode::Auto,
            }))
        );
    }

    #[test]
    fn parses_glyph_overrides() {
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "--ascii"])),
            Ok(Command::Run(Options {
                pid: None,
                glyph_mode: GlyphMode::Ascii,
            }))
        );
        assert_eq!(
            parse_args(os_args(&["pstree.exe", "--unicode", "123"])),
            Ok(Command::Run(Options {
                pid: Some(123),
                glyph_mode: GlyphMode::Unicode,
            }))
        );
    }

    #[test]
    fn rejects_conflicting_glyphs() {
        assert!(parse_args(os_args(&["pstree.exe", "--ascii", "--unicode"])).is_err());
    }

    #[test]
    fn rejects_invalid_pid_and_unknown_options() {
        assert!(parse_args(os_args(&["pstree.exe", "0"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "abc"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "--wat"])).is_err());
        assert!(parse_args(os_args(&["pstree.exe", "1", "2"])).is_err());
    }
}
