use std::process::{Command, Output};

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pstree"))
        .args(args)
        .output()
        .expect("pstree binary should run")
}

fn assert_status(args: &[&str], expected: i32) -> Output {
    let output = run_cli(args);
    assert_eq!(output.status.code(), Some(expected), "args: {args:?}");
    output
}

#[test]
fn help_aliases_exit_successfully() {
    for args in [["--help"].as_slice(), ["-?"].as_slice()] {
        let output = assert_status(args, 0);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage: pstree.exe [OPTION]... [PID]"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn version_exits_successfully() {
    let output = assert_status(&["--version"], 0);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("pstree-windows {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_and_conflicting_options_exit_with_usage_error() {
    for args in [
        &["--wat"][..],
        &["0"][..],
        &["1", "2"][..],
        &["--ascii", "--unicode"][..],
        &["-tT"][..],
    ] {
        let output = assert_status(args, 2);
        assert!(String::from_utf8_lossy(&output.stderr).contains("pstree:"));
    }
}

#[test]
fn deferred_and_unsupported_options_never_succeed_silently() {
    for args in [
        &["-h"][..],
        &["-pn"][..],
        &["--show-pids"][..],
        &["-a"][..],
        &["--color=age"][..],
        &["-H"][..],
        &["--ns-sort=pid"][..],
    ] {
        let output = assert_status(args, 2);
        assert!(String::from_utf8_lossy(&output.stderr).contains("not supported"));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn runtime_failure_uses_status_one() {
    assert_status(&["4294967295"], 1);
}
