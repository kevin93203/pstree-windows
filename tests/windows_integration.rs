#![cfg(windows)]

use std::process::{Child, Command, Output};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use pstree_windows::{collect_processes, collect_snapshot};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn snapshot_contains_the_test_process() {
    let processes = collect_processes().expect("Toolhelp32 snapshot should succeed");
    let current_pid = std::process::id();

    let current = processes
        .iter()
        .find(|process| process.pid == current_pid)
        .expect("the test process should be present in the snapshot");
    assert!(!current.name.is_empty());
}

#[test]
fn snapshot_reports_a_spawned_child_parent() {
    let child = Command::new("cmd.exe")
        .args(["/C", "ping 127.0.0.1 -n 6 > nul"])
        .spawn()
        .expect("cmd.exe should be available on Windows");
    let child_pid = child.id();
    let _child_guard = ChildGuard(child);
    let current_pid = std::process::id();
    let deadline = Instant::now() + Duration::from_secs(2);

    loop {
        let processes = collect_processes().expect("Toolhelp32 snapshot should succeed");
        if processes
            .iter()
            .any(|process| process.pid == child_pid && process.parent_pid == current_pid)
        {
            return;
        }

        if Instant::now() >= deadline {
            panic!("spawned child {child_pid} was not found with parent {current_pid}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn snapshot_contains_a_thread_owned_by_the_test_process() {
    let (ready_sender, ready_receiver) = mpsc::channel();
    let (stop_sender, stop_receiver) = mpsc::channel();
    let thread = thread::spawn(move || {
        ready_sender
            .send(current_thread_id())
            .expect("test thread should report its id");
        let _ = stop_receiver.recv();
    });

    let spawned_tid = ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("test thread should start");
    let snapshot = collect_snapshot(false).expect("Toolhelp32 snapshot should succeed");
    let current_pid = std::process::id();
    let process_name = snapshot
        .processes
        .iter()
        .find(|process| process.pid == current_pid)
        .expect("the test process should be present in the snapshot")
        .name
        .clone();

    let reported_thread = snapshot
        .threads
        .iter()
        .find(|thread| thread.owner_pid == current_pid && thread.tid == spawned_tid)
        .expect("the spawned non-main thread should be present in the snapshot");
    assert_eq!(reported_thread.name.as_deref(), Some(process_name.as_str()));
    let _ = stop_sender.send(());
    thread.join().expect("test thread should stop");
}

#[test]
fn thread_description_lookup_is_best_effort() {
    let (ready_sender, ready_receiver) = mpsc::channel();
    let (stop_sender, stop_receiver) = mpsc::channel();
    let thread = thread::spawn(move || {
        let tid = current_thread_id();
        ready_sender
            .send((tid, set_current_thread_description("pstree-test-thread")))
            .expect("test thread should report its state");
        let _ = stop_receiver.recv();
    });

    let (reported_tid, _description_was_set) = ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("test thread should start");
    let snapshot = collect_snapshot(true).expect("thread name lookup should be best effort");
    let reported_thread = snapshot
        .threads
        .iter()
        .find(|thread| thread.owner_pid == std::process::id() && thread.tid == reported_tid)
        .expect("the spawned thread should be present in the snapshot");
    assert!(reported_thread.name.is_some());

    let _ = stop_sender.send(());
    thread.join().expect("test thread should stop");
}

#[test]
fn p1_cli_options_execute_successfully() {
    let pid = std::process::id().to_string();
    for args in [
        vec!["-p".to_owned(), pid.clone()],
        vec!["-n".to_owned(), pid.clone()],
        vec!["-c".to_owned(), pid.clone()],
        vec!["-t".to_owned(), pid.clone()],
        vec!["-T".to_owned(), pid.clone()],
        vec!["-s".to_owned(), "-p".to_owned(), pid.clone()],
    ] {
        let output = run_cli(&args);
        assert_eq!(output.status.code(), Some(0), "args: {args:?}");
        assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    }

    let output = run_cli(&["-p".to_owned(), pid.clone()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("({pid})")));
    assert!(!stdout.contains("[TID="));

    let output = run_cli(&["-T".to_owned(), pid]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.lines().any(|line| line.contains('{')));
}

fn run_cli(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pstree"))
        .args(args)
        .output()
        .expect("pstree binary should run")
}

#[cfg(windows)]
fn set_current_thread_description(description: &str) -> bool {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadDescription};
    use windows::core::PCWSTR;

    let mut value = description.encode_utf16().collect::<Vec<_>>();
    value.push(0);
    unsafe { SetThreadDescription(GetCurrentThread(), PCWSTR(value.as_ptr())).is_ok() }
}

#[cfg(windows)]
fn current_thread_id() -> u32 {
    use windows::Win32::System::Threading::GetCurrentThreadId;

    unsafe { GetCurrentThreadId() }
}
