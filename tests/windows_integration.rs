#![cfg(windows)]

use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use pstree_windows::collect_processes;

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
