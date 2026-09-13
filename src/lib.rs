use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
}

#[cfg(windows)]
pub fn collect_processes() -> windows::core::Result<Vec<ProcessInfo>> {
    use std::mem::size_of;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    struct Snapshot(HANDLE);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let snapshot = Snapshot(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut processes = Vec::new();

    unsafe { Process32FirstW(snapshot.0, &mut entry)? };
    loop {
        processes.push(ProcessInfo {
            pid: entry.th32ProcessID,
            parent_pid: entry.th32ParentProcessID,
            name: process_name(&entry.szExeFile),
        });

        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }

    Ok(processes)
}

#[cfg(windows)]
fn process_name(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphSet {
    Ascii,
    Unicode,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenderError {
    ProcessNotFound(u32),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProcessNotFound(pid) => write!(f, "process {pid} was not found"),
        }
    }
}

impl std::error::Error for RenderError {}

#[derive(Debug, Default)]
pub struct ProcessTree {
    nodes: BTreeMap<u32, ProcessInfo>,
    children: BTreeMap<u32, Vec<u32>>,
    roots: Vec<u32>,
}

struct RenderState {
    glyph_set: GlyphSet,
    visited: BTreeSet<u32>,
    output: String,
}

impl ProcessTree {
    pub fn from_processes(processes: impl IntoIterator<Item = ProcessInfo>) -> Self {
        let nodes = processes
            .into_iter()
            .map(|process| (process.pid, process))
            .collect::<BTreeMap<_, _>>();

        let mut children = BTreeMap::<u32, Vec<u32>>::new();
        let mut roots = Vec::new();

        for (&pid, process) in &nodes {
            if process.parent_pid == pid || !nodes.contains_key(&process.parent_pid) {
                roots.push(pid);
            } else {
                children.entry(process.parent_pid).or_default().push(pid);
            }
        }

        Self {
            nodes,
            children,
            roots,
        }
    }

    pub fn contains(&self, pid: u32) -> bool {
        self.nodes.contains_key(&pid)
    }

    pub fn roots(&self) -> &[u32] {
        &self.roots
    }

    pub fn children_of(&self, pid: u32) -> &[u32] {
        self.children.get(&pid).map_or(&[], Vec::as_slice)
    }

    pub fn process(&self, pid: u32) -> Option<&ProcessInfo> {
        self.nodes.get(&pid)
    }

    pub fn render(&self, root: Option<u32>, glyph_set: GlyphSet) -> Result<String, RenderError> {
        let roots = match root {
            Some(pid) if !self.contains(pid) => return Err(RenderError::ProcessNotFound(pid)),
            Some(pid) => vec![pid],
            None => self.roots.clone(),
        };

        let mut state = RenderState {
            glyph_set,
            visited: BTreeSet::new(),
            output: String::new(),
        };

        for pid in roots {
            self.render_node(pid, "", None, true, &mut state);
        }

        Ok(state.output)
    }

    fn render_node(
        &self,
        pid: u32,
        prefix: &str,
        connector: Option<&str>,
        is_last: bool,
        state: &mut RenderState,
    ) {
        if !state.visited.insert(pid) {
            return;
        }

        if let Some(connector) = connector {
            state.output.push_str(prefix);
            state.output.push_str(connector);
        }

        if let Some(process) = self.process(pid) {
            let _ = writeln!(&mut state.output, "{}({})", process.name, process.pid);
        }

        let children = self.children_of(pid);
        let child_prefix = if connector.is_none() {
            String::new()
        } else {
            let mut value = String::with_capacity(prefix.len() + 2);
            value.push_str(prefix);
            value.push_str(state.glyph_set.continuation(is_last));
            value
        };

        for (index, child_pid) in children.iter().copied().enumerate() {
            let child_is_last = index + 1 == children.len();
            self.render_node(
                child_pid,
                &child_prefix,
                Some(state.glyph_set.connector(child_is_last)),
                child_is_last,
                state,
            );
        }
    }
}

impl GlyphSet {
    fn connector(self, is_last: bool) -> &'static str {
        match self {
            Self::Ascii => {
                if is_last {
                    "`-"
                } else {
                    "|-"
                }
            }
            Self::Unicode => {
                if is_last {
                    "└─"
                } else {
                    "├─"
                }
            }
        }
    }

    fn continuation(self, parent_is_last: bool) -> &'static str {
        match self {
            Self::Ascii => {
                if parent_is_last {
                    "  "
                } else {
                    "| "
                }
            }
            Self::Unicode => {
                if parent_is_last {
                    "  "
                } else {
                    "│ "
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, parent_pid: u32, name: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent_pid,
            name: name.to_owned(),
        }
    }

    #[test]
    fn empty_tree_renders_empty_output() {
        let tree = ProcessTree::from_processes([]);
        assert_eq!(tree.render(None, GlyphSet::Ascii).unwrap(), "");
    }

    #[test]
    fn roots_and_children_are_sorted_by_pid() {
        let tree = ProcessTree::from_processes([
            process(30, 10, "child-b"),
            process(20, 0, "root-b"),
            process(10, 0, "root-a"),
            process(40, 10, "child-c"),
            process(25, 10, "child-a"),
        ]);

        assert_eq!(tree.roots(), &[10, 20]);
        assert_eq!(tree.children_of(10), &[25, 30, 40]);
    }

    #[test]
    fn missing_and_self_parents_become_roots() {
        let tree = ProcessTree::from_processes([
            process(10, 999, "orphan"),
            process(20, 20, "self-parent"),
        ]);

        assert_eq!(tree.roots(), &[10, 20]);
    }

    #[test]
    fn ascii_render_uses_expected_prefixes() {
        let tree = ProcessTree::from_processes([
            process(1, 0, "root"),
            process(2, 1, "first"),
            process(3, 1, "last"),
            process(4, 2, "grandchild"),
        ]);

        assert_eq!(
            tree.render(None, GlyphSet::Ascii).unwrap(),
            "root(1)\n|-first(2)\n| `-grandchild(4)\n`-last(3)\n"
        );
    }

    #[test]
    fn unicode_render_uses_expected_prefixes() {
        let tree = ProcessTree::from_processes([
            process(1, 0, "root"),
            process(2, 1, "first"),
            process(3, 1, "last"),
            process(4, 2, "grandchild"),
        ]);

        assert_eq!(
            tree.render(None, GlyphSet::Unicode).unwrap(),
            "root(1)\n├─first(2)\n│ └─grandchild(4)\n└─last(3)\n"
        );
    }

    #[test]
    fn selected_pid_renders_only_that_subtree() {
        let tree = ProcessTree::from_processes([
            process(1, 0, "root"),
            process(2, 1, "selected"),
            process(3, 2, "descendant"),
            process(4, 1, "sibling"),
        ]);

        assert_eq!(
            tree.render(Some(2), GlyphSet::Ascii).unwrap(),
            "selected(2)\n`-descendant(3)\n"
        );
    }

    #[test]
    fn unknown_pid_is_an_error() {
        let tree = ProcessTree::from_processes([process(1, 0, "root")]);
        assert_eq!(
            tree.render(Some(99), GlyphSet::Ascii),
            Err(RenderError::ProcessNotFound(99))
        );
    }

    #[test]
    fn duplicate_names_remain_distinguishable_by_pid() {
        let tree = ProcessTree::from_processes([process(1, 0, "worker"), process(2, 1, "worker")]);

        assert_eq!(
            tree.render(None, GlyphSet::Ascii).unwrap(),
            "worker(1)\n`-worker(2)\n"
        );
    }
}
