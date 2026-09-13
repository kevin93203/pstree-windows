use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadInfo {
    pub tid: u32,
    pub owner_pid: u32,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemSnapshot {
    pub processes: Vec<ProcessInfo>,
    pub threads: Vec<ThreadInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderOptions {
    pub show_pids: bool,
    pub numeric_sort: bool,
    pub compact: bool,
    pub show_threads: bool,
    pub show_parents: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            show_pids: false,
            numeric_sort: false,
            compact: true,
            show_threads: true,
            show_parents: false,
        }
    }
}

#[cfg(windows)]
pub fn collect_snapshot(include_thread_names: bool) -> windows::core::Result<SystemSnapshot> {
    use std::mem::size_of;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };

    struct Snapshot(HANDLE);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let snapshot =
        Snapshot(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS | TH32CS_SNAPTHREAD, 0)? });

    let mut process_entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut processes = Vec::new();

    unsafe { Process32FirstW(snapshot.0, &mut process_entry)? };
    loop {
        processes.push(ProcessInfo {
            pid: process_entry.th32ProcessID,
            parent_pid: process_entry.th32ParentProcessID,
            name: process_name(&process_entry.szExeFile),
        });

        if unsafe { Process32NextW(snapshot.0, &mut process_entry) }.is_err() {
            break;
        }
    }

    let process_names = processes
        .iter()
        .map(|process| (process.pid, process.name.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut threads = Vec::new();
    let mut thread_entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };

    if unsafe { Thread32First(snapshot.0, &mut thread_entry) }.is_ok() {
        loop {
            let owner_pid = thread_entry.th32OwnerProcessID;
            let tid = thread_entry.th32ThreadID;
            if let Some(owner_name) = process_names.get(&owner_pid) {
                let name = if include_thread_names {
                    thread_description(tid).or_else(|| Some(owner_name.clone()))
                } else {
                    Some(owner_name.clone())
                };

                threads.push(ThreadInfo {
                    tid,
                    owner_pid,
                    name,
                });
            }

            if unsafe { Thread32Next(snapshot.0, &mut thread_entry) }.is_err() {
                break;
            }
        }
    }

    let primary_thread_ids = primary_thread_ids(&threads);
    threads.retain(|thread| primary_thread_ids.get(&thread.owner_pid) != Some(&thread.tid));
    threads.sort_by_key(|thread| thread.tid);
    Ok(SystemSnapshot { processes, threads })
}

#[cfg(windows)]
pub fn collect_processes() -> windows::core::Result<Vec<ProcessInfo>> {
    Ok(collect_snapshot(false)?.processes)
}

#[cfg(windows)]
fn process_name(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

#[cfg(windows)]
fn thread_description(tid: u32) -> Option<String> {
    use std::mem::transmute;
    use std::slice;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Threading::{OpenThread, THREAD_QUERY_LIMITED_INFORMATION};
    use windows::core::{PCSTR, PWSTR, w};

    type GetThreadDescription =
        unsafe extern "system" fn(HANDLE, *mut PWSTR) -> windows::core::HRESULT;

    static GET_THREAD_DESCRIPTION: OnceLock<Option<GetThreadDescription>> = OnceLock::new();
    let get_description = (*GET_THREAD_DESCRIPTION.get_or_init(|| unsafe {
        let module = GetModuleHandleW(w!("kernel32.dll")).ok()?;
        let address = GetProcAddress(module, PCSTR(c"GetThreadDescription".as_ptr().cast()))?;
        Some(transmute::<
            unsafe extern "system" fn() -> isize,
            GetThreadDescription,
        >(address))
    }))?;

    let handle = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, tid).ok()? };
    struct ThreadHandle(HANDLE);
    impl Drop for ThreadHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let _handle = ThreadHandle(handle);

    let mut description = PWSTR(std::ptr::null_mut());
    let result = unsafe { get_description(handle, &mut description) };
    let value = if result.is_ok() && !description.0.is_null() {
        let mut length = 0;
        unsafe {
            while *description.0.add(length) != 0 {
                length += 1;
            }
            Some(String::from_utf16_lossy(slice::from_raw_parts(
                description.0,
                length,
            )))
        }
    } else {
        None
    };

    if !description.0.is_null() {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(description.0.cast())));
        }
    }

    value.filter(|value| !value.is_empty())
}

#[cfg(windows)]
fn primary_thread_ids(threads: &[ThreadInfo]) -> BTreeMap<u32, u32> {
    let mut candidates = BTreeMap::<u32, Option<(u64, u32)>>::new();

    for thread in threads {
        if let Some(creation_time) = thread_creation_time(thread.tid) {
            let candidate = candidates.entry(thread.owner_pid).or_default();
            if candidate.is_none_or(|(old_time, _)| creation_time < old_time) {
                *candidate = Some((creation_time, thread.tid));
            }
        }
    }

    candidates
        .into_iter()
        .filter_map(|(owner_pid, candidate)| candidate.map(|(_, tid)| (owner_pid, tid)))
        .collect()
}

#[cfg(windows)]
fn thread_creation_time(tid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Threading::{GetThreadTimes, OpenThread, THREAD_QUERY_INFORMATION};

    let handle = unsafe { OpenThread(THREAD_QUERY_INFORMATION, false, tid).ok()? };
    struct ThreadHandle(HANDLE);
    impl Drop for ThreadHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let _handle = ThreadHandle(handle);

    let mut creation_time = FILETIME::default();
    let mut exit_time = FILETIME::default();
    let mut kernel_time = FILETIME::default();
    let mut user_time = FILETIME::default();
    unsafe {
        GetThreadTimes(
            handle,
            &mut creation_time,
            &mut exit_time,
            &mut kernel_time,
            &mut user_time,
        )
        .ok()?;
    }

    Some((u64::from(creation_time.dwHighDateTime) << 32) | u64::from(creation_time.dwLowDateTime))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphSet {
    Ascii,
    Unicode,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenderError {
    ProcessNotFound(u32),
    ParentsRequirePid,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProcessNotFound(pid) => write!(f, "process {pid} was not found"),
            Self::ParentsRequirePid => f.write_str("--show-parents requires a PID"),
        }
    }
}

impl std::error::Error for RenderError {}

#[derive(Debug, Default)]
pub struct ProcessTree {
    nodes: BTreeMap<u32, ProcessInfo>,
    children: BTreeMap<u32, Vec<u32>>,
    roots: Vec<u32>,
    threads: BTreeMap<u32, Vec<ThreadInfo>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BranchSignature {
    name: String,
    threads: Vec<Option<String>>,
    children: Vec<BranchSignature>,
}

struct RenderState {
    glyph_set: GlyphSet,
    options: RenderOptions,
    visited: BTreeSet<u32>,
    lines: Vec<Vec<char>>,
}

impl RenderState {
    fn new_line(&mut self) -> usize {
        self.lines.push(Vec::new());
        self.lines.len() - 1
    }

    fn put(&mut self, line: usize, column: usize, value: &str) {
        let characters = &mut self.lines[line];
        if characters.len() < column {
            characters.resize(column, ' ');
        }
        for (offset, character) in value.chars().enumerate() {
            let position = column + offset;
            if characters.len() <= position {
                characters.push(character);
            } else {
                characters[position] = character;
            }
        }
    }

    fn finish(self) -> String {
        let mut output = String::new();
        for mut line in self.lines {
            while line.last() == Some(&' ') {
                line.pop();
            }
            output.extend(line);
            output.push('\n');
        }
        output
    }
}

enum RenderEntry {
    Threads(Vec<ThreadInfo>),
    Processes(Vec<u32>),
}

impl ProcessTree {
    pub fn from_processes(processes: impl IntoIterator<Item = ProcessInfo>) -> Self {
        Self::from_snapshot(SystemSnapshot {
            processes: processes.into_iter().collect(),
            threads: Vec::new(),
        })
    }

    pub fn from_snapshot(snapshot: SystemSnapshot) -> Self {
        let nodes = snapshot
            .processes
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

        let mut threads = BTreeMap::<u32, Vec<ThreadInfo>>::new();
        for thread in snapshot.threads {
            if nodes.contains_key(&thread.owner_pid) {
                threads.entry(thread.owner_pid).or_default().push(thread);
            }
        }
        for owner_threads in threads.values_mut() {
            owner_threads.sort_by_key(|thread| thread.tid);
        }

        let mut reachable = BTreeSet::new();
        for &root in &roots {
            mark_reachable(root, &children, &mut reachable);
        }
        for pid in nodes.keys().copied().collect::<Vec<_>>() {
            if !reachable.contains(&pid) {
                roots.push(pid);
                mark_reachable(pid, &children, &mut reachable);
            }
        }
        roots.sort_unstable();
        roots.dedup();

        Self {
            nodes,
            children,
            roots,
            threads,
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

    pub fn threads_of(&self, pid: u32) -> &[ThreadInfo] {
        self.threads.get(&pid).map_or(&[], Vec::as_slice)
    }

    pub fn process(&self, pid: u32) -> Option<&ProcessInfo> {
        self.nodes.get(&pid)
    }

    pub fn render(&self, root: Option<u32>, glyph_set: GlyphSet) -> Result<String, RenderError> {
        self.render_with_options(root, glyph_set, &RenderOptions::legacy())
    }

    pub fn render_with_options(
        &self,
        root: Option<u32>,
        glyph_set: GlyphSet,
        options: &RenderOptions,
    ) -> Result<String, RenderError> {
        if options.show_parents && root.is_none() {
            return Err(RenderError::ParentsRequirePid);
        }
        if let Some(pid) = root.filter(|pid| !self.contains(*pid)) {
            return Err(RenderError::ProcessNotFound(pid));
        }

        let mut state = RenderState {
            glyph_set,
            options: *options,
            visited: BTreeSet::new(),
            lines: Vec::new(),
        };

        match root {
            Some(pid) if options.show_parents => self.render_ancestor_chain(pid, &mut state),
            Some(pid) => {
                self.render_process_group(&[pid], &mut state);
            }
            None => {
                for group in self.process_groups(self.sorted_roots(options), options) {
                    self.render_process_group(&group, &mut state);
                }
            }
        }

        Ok(state.finish())
    }

    fn render_ancestor_chain(&self, pid: u32, state: &mut RenderState) {
        let chain = self.ancestor_chain(pid);
        let target_index = chain.len() - 1;
        let line = state.new_line();
        let mut column = 0;

        for &ancestor in &chain[..target_index] {
            state.visited.insert(ancestor);
            let label = self.process_label(ancestor, state);
            state.put(line, column, &label);
            column += label.chars().count();
            state.put(line, column, state.glyph_set.single_connector());
            column += state.glyph_set.single_connector().chars().count();
        }

        self.draw_process_group(&[chain[target_index]], column, line, &[], state);
    }

    fn ancestor_chain(&self, pid: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = pid;

        while seen.insert(current) {
            chain.push(current);
            let Some(process) = self.process(current) else {
                break;
            };
            if process.parent_pid == current || !self.contains(process.parent_pid) {
                break;
            }
            current = process.parent_pid;
        }

        chain.reverse();
        chain
    }

    fn render_process_group(&self, pids: &[u32], state: &mut RenderState) {
        let line = state.new_line();
        self.draw_process_group(pids, 0, line, &[], state);
    }

    fn draw_process_group(
        &self,
        pids: &[u32],
        column: usize,
        line: usize,
        context: &[usize],
        state: &mut RenderState,
    ) {
        let group = pids
            .iter()
            .copied()
            .filter(|pid| self.nodes.contains_key(pid) && state.visited.insert(*pid))
            .collect::<Vec<_>>();
        let Some(&representative) = group.first() else {
            return;
        };

        let label = if group.len() > 1 && !state.options.show_pids {
            format!("{}*[{}]", group.len(), self.nodes[&representative].name)
        } else {
            self.process_label(representative, state)
        };
        state.put(line, column, &label);

        let entries = self
            .child_entries(representative, &state.options)
            .into_iter()
            .filter(|entry| self.entry_is_visible(entry, state))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return;
        }

        let branch_column = column + label.chars().count();
        let multiple_entries = entries.len() > 1;

        state.put(
            line,
            branch_column,
            if multiple_entries {
                state.glyph_set.branch_connector()
            } else {
                state.glyph_set.single_connector()
            },
        );
        let first_context = if multiple_entries {
            let mut child_context = context.to_vec();
            child_context.push(branch_column + state.glyph_set.branch_vertical_offset());
            child_context
        } else {
            context.to_vec()
        };
        self.draw_entry(
            &entries[0],
            branch_column + state.glyph_set.child_connector_width(),
            line,
            &first_context,
            state,
        );

        for (index, entry) in entries.iter().enumerate().skip(1) {
            let child_line = state.new_line();
            self.draw_context(child_line, context, state);
            let is_last = index + 1 == entries.len();
            let connector = state.glyph_set.connector(is_last);
            let junction_column = branch_column + state.glyph_set.branch_vertical_offset();
            state.put(child_line, junction_column, connector);
            let mut child_context = context.to_vec();
            if !is_last {
                child_context.push(branch_column + state.glyph_set.branch_vertical_offset());
            }
            self.draw_entry(
                entry,
                junction_column + connector.chars().count(),
                child_line,
                &child_context,
                state,
            );
        }
    }

    fn draw_entry(
        &self,
        entry: &RenderEntry,
        column: usize,
        line: usize,
        context: &[usize],
        state: &mut RenderState,
    ) {
        match entry {
            RenderEntry::Threads(threads) => self.draw_thread_group(threads, column, line, state),
            RenderEntry::Processes(pids) => {
                self.draw_process_group(pids, column, line, context, state)
            }
        }
    }

    fn draw_context(&self, line: usize, context: &[usize], state: &mut RenderState) {
        for &column in context {
            state.put(line, column, state.glyph_set.vertical());
        }
    }

    fn entry_is_visible(&self, entry: &RenderEntry, state: &RenderState) -> bool {
        match entry {
            RenderEntry::Threads(threads) => !threads.is_empty(),
            RenderEntry::Processes(pids) => pids
                .iter()
                .any(|pid| self.nodes.contains_key(pid) && !state.visited.contains(pid)),
        }
    }

    fn draw_thread_group(
        &self,
        threads: &[ThreadInfo],
        column: usize,
        line: usize,
        state: &mut RenderState,
    ) {
        let Some(thread) = threads.first() else {
            return;
        };

        let name = self.thread_name(thread);
        if threads.len() > 1 && state.options.compact && !state.options.show_pids {
            state.put(line, column, &format!("{}*[{{{name}}}]", threads.len()));
        } else {
            let label = if state.options.show_pids {
                format!("{{{name}}}({})", thread.tid)
            } else {
                format!("{{{name}}}")
            };
            state.put(line, column, &label);
        }
    }

    fn thread_name<'a>(&'a self, thread: &'a ThreadInfo) -> &'a str {
        thread
            .name
            .as_deref()
            .or_else(|| {
                self.nodes
                    .get(&thread.owner_pid)
                    .map(|process| process.name.as_str())
            })
            .unwrap_or("?")
    }

    fn process_label(&self, pid: u32, state: &RenderState) -> String {
        let process = &self.nodes[&pid];
        if state.options.show_pids {
            format!("{}({})", process.name, process.pid)
        } else {
            process.name.clone()
        }
    }

    fn child_entries(&self, pid: u32, options: &RenderOptions) -> Vec<RenderEntry> {
        let mut entries = Vec::new();

        if options.show_threads {
            let threads = self.sorted_threads(pid, options);
            let mut index = 0;
            while index < threads.len() {
                let thread = &threads[index];
                let mut end = index + 1;
                if options.compact && !options.show_pids {
                    while end < threads.len()
                        && self.thread_name(&threads[end]) == self.thread_name(thread)
                    {
                        end += 1;
                    }
                }
                entries.push(RenderEntry::Threads(threads[index..end].to_vec()));
                index = end;
            }
        }

        entries.extend(
            self.process_groups(self.sorted_children(pid, options), options)
                .into_iter()
                .map(RenderEntry::Processes),
        );
        entries
    }

    fn sorted_roots(&self, options: &RenderOptions) -> Vec<u32> {
        self.sorted_processes(&self.roots, options)
    }

    fn sorted_children(&self, pid: u32, options: &RenderOptions) -> Vec<u32> {
        self.children.get(&pid).map_or_else(Vec::new, |children| {
            self.sorted_processes(children, options)
        })
    }

    fn sorted_threads(&self, pid: u32, options: &RenderOptions) -> Vec<ThreadInfo> {
        let mut threads = self.threads_of(pid).to_vec();
        if options.numeric_sort {
            threads.sort_unstable_by_key(|thread| thread.tid);
        } else {
            threads.sort_unstable_by(|left, right| {
                self.thread_name(left)
                    .cmp(self.thread_name(right))
                    .then_with(|| left.tid.cmp(&right.tid))
            });
        }
        threads
    }

    fn sorted_processes(&self, pids: &[u32], options: &RenderOptions) -> Vec<u32> {
        let mut sorted = pids.to_vec();
        if options.numeric_sort {
            sorted.sort_unstable();
        } else {
            sorted.sort_unstable_by(|left, right| {
                self.nodes[left]
                    .name
                    .cmp(&self.nodes[right].name)
                    .then_with(|| left.cmp(right))
            });
        }
        sorted
    }

    fn process_groups(&self, pids: Vec<u32>, options: &RenderOptions) -> Vec<Vec<u32>> {
        if !options.compact || options.show_pids {
            return pids.into_iter().map(|pid| vec![pid]).collect();
        }

        let mut groups: Vec<Vec<u32>> = Vec::new();
        let mut previous_signature = None;
        for pid in pids {
            let signature = self.branch_signature(pid, options, &mut BTreeSet::new());
            if previous_signature.as_ref() == Some(&signature) {
                groups
                    .last_mut()
                    .expect("a previous signature has a group")
                    .push(pid);
            } else {
                previous_signature = Some(signature);
                groups.push(vec![pid]);
            }
        }
        groups
    }

    fn branch_signature(
        &self,
        pid: u32,
        options: &RenderOptions,
        path: &mut BTreeSet<u32>,
    ) -> BranchSignature {
        if !path.insert(pid) {
            return BranchSignature {
                name: "<cycle>".to_owned(),
                threads: Vec::new(),
                children: Vec::new(),
            };
        }

        let threads = if options.show_threads {
            self.sorted_threads(pid, options)
                .iter()
                .map(|thread| Some(self.thread_name(thread).to_owned()))
                .collect()
        } else {
            Vec::new()
        };
        let children = self
            .sorted_children(pid, options)
            .into_iter()
            .map(|child| self.branch_signature(child, options, path))
            .collect();
        path.remove(&pid);

        BranchSignature {
            name: self.nodes[&pid].name.clone(),
            threads,
            children,
        }
    }
}

fn mark_reachable(pid: u32, children: &BTreeMap<u32, Vec<u32>>, reachable: &mut BTreeSet<u32>) {
    let mut pending = vec![pid];
    while let Some(current) = pending.pop() {
        if !reachable.insert(current) {
            continue;
        }
        if let Some(children) = children.get(&current) {
            pending.extend(children.iter().copied());
        }
    }
}

impl RenderOptions {
    fn legacy() -> Self {
        Self {
            show_pids: true,
            numeric_sort: true,
            compact: false,
            show_threads: false,
            show_parents: false,
        }
    }
}

impl GlyphSet {
    fn branch_connector(self) -> &'static str {
        match self {
            Self::Ascii => "-+-",
            Self::Unicode => "─┬─",
        }
    }

    fn single_connector(self) -> &'static str {
        match self {
            Self::Ascii => "---",
            Self::Unicode => "───",
        }
    }

    fn child_connector_width(self) -> usize {
        self.single_connector().chars().count()
    }

    fn branch_vertical_offset(self) -> usize {
        match self {
            Self::Ascii | Self::Unicode => 1,
        }
    }

    fn vertical(self) -> &'static str {
        match self {
            Self::Ascii => "|",
            Self::Unicode => "│",
        }
    }

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

    fn thread(tid: u32, owner_pid: u32, name: Option<&str>) -> ThreadInfo {
        ThreadInfo {
            tid,
            owner_pid,
            name: name.map(str::to_owned),
        }
    }

    fn render(tree: &ProcessTree, root: Option<u32>, options: RenderOptions) -> String {
        tree.render_with_options(root, GlyphSet::Ascii, &options)
            .unwrap()
    }

    #[test]
    fn empty_tree_renders_empty_output() {
        let tree = ProcessTree::from_processes([]);
        assert_eq!(tree.render(None, GlyphSet::Ascii).unwrap(), "");
    }

    #[test]
    fn roots_and_children_are_sorted_by_pid_for_legacy_render() {
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
    fn process_cycles_get_a_stable_fallback_root() {
        let tree =
            ProcessTree::from_processes([process(20, 10, "second"), process(10, 20, "first")]);

        assert_eq!(tree.roots(), &[10]);
        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    show_threads: false,
                    compact: false,
                    ..RenderOptions::default()
                }
            ),
            "first---second\n"
        );
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
            "root(1)-+-first(2)---grandchild(4)\n        `-last(3)\n"
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
            "root(1)─┬─first(2)───grandchild(4)\n        └─last(3)\n"
        );
    }

    #[test]
    fn horizontal_layout_preserves_nested_branch_alignment() {
        let tree = ProcessTree::from_processes([
            process(1, 0, "root"),
            process(2, 1, "first"),
            process(3, 1, "last"),
            process(4, 2, "grand-a"),
            process(5, 2, "grand-b"),
        ]);

        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    compact: false,
                    show_threads: false,
                    ..RenderOptions::default()
                }
            ),
            "root-+-first-+-grand-a\n     |       `-grand-b\n     `-last\n"
        );
    }

    #[test]
    fn selected_pid_renders_only_that_subtree_without_parents() {
        let tree = ProcessTree::from_processes([
            process(1, 0, "root"),
            process(2, 1, "selected"),
            process(3, 2, "descendant"),
            process(4, 1, "sibling"),
        ]);

        assert_eq!(
            render(
                &tree,
                Some(2),
                RenderOptions {
                    show_threads: false,
                    compact: false,
                    ..RenderOptions::default()
                }
            ),
            "selected---descendant\n"
        );
    }

    #[test]
    fn show_parents_renders_process_only_ancestor_chain() {
        let tree = ProcessTree::from_snapshot(SystemSnapshot {
            processes: vec![
                process(1, 0, "root"),
                process(2, 1, "parent"),
                process(3, 2, "selected"),
                process(4, 3, "child"),
            ],
            threads: vec![thread(40, 2, Some("parent-thread")), thread(41, 3, None)],
        });

        assert_eq!(
            render(
                &tree,
                Some(3),
                RenderOptions {
                    show_parents: true,
                    ..RenderOptions::default()
                }
            ),
            "root---parent---selected-+-{selected}\n                         `-child\n"
        );
    }

    #[test]
    fn show_parents_requires_a_pid() {
        let tree = ProcessTree::from_processes([process(1, 0, "root")]);
        assert_eq!(
            tree.render_with_options(
                None,
                GlyphSet::Ascii,
                &RenderOptions {
                    show_parents: true,
                    ..RenderOptions::default()
                }
            ),
            Err(RenderError::ParentsRequirePid)
        );
    }

    #[test]
    fn default_process_sort_is_name_then_pid() {
        let tree = ProcessTree::from_processes([
            process(30, 0, "alpha"),
            process(20, 0, "zeta"),
            process(10, 0, "beta"),
        ]);

        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    show_threads: false,
                    compact: false,
                    ..RenderOptions::default()
                }
            ),
            "alpha\nbeta\nzeta\n"
        );
        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    numeric_sort: true,
                    show_threads: false,
                    compact: false,
                    ..RenderOptions::default()
                }
            ),
            "beta\nzeta\nalpha\n"
        );
    }

    #[test]
    fn pids_are_optional_and_disable_process_compaction() {
        let tree = ProcessTree::from_processes([
            process(1, 0, "root"),
            process(2, 1, "worker"),
            process(3, 1, "worker"),
        ]);

        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    show_threads: false,
                    ..RenderOptions::default()
                }
            ),
            "root---2*[worker]\n"
        );
        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    show_pids: true,
                    compact: false,
                    show_threads: false,
                    ..RenderOptions::default()
                }
            ),
            "root(1)-+-worker(2)\n        `-worker(3)\n"
        );
    }

    #[test]
    fn compact_false_disables_process_compaction() {
        let tree = ProcessTree::from_processes([
            process(1, 0, "root"),
            process(2, 1, "worker"),
            process(3, 1, "worker"),
        ]);

        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    compact: false,
                    show_threads: false,
                    ..RenderOptions::default()
                }
            ),
            "root-+-worker\n     `-worker\n"
        );
    }

    #[test]
    fn threads_are_sorted_by_name_or_tid_and_rendered_with_owner_fallback() {
        let tree = ProcessTree::from_snapshot(SystemSnapshot {
            processes: vec![process(1, 0, "root")],
            threads: vec![thread(30, 1, None), thread(20, 1, Some("Worker"))],
        });

        assert_eq!(
            render(&tree, None, RenderOptions::default()),
            "root-+-{Worker}\n     `-{root}\n"
        );
        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    show_pids: true,
                    ..RenderOptions::default()
                }
            ),
            "root(1)-+-{Worker}(20)\n        `-{root}(30)\n"
        );
        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    show_threads: false,
                    ..RenderOptions::default()
                }
            ),
            "root\n"
        );
    }

    #[test]
    fn named_threads_group_without_pids() {
        let tree = ProcessTree::from_snapshot(SystemSnapshot {
            processes: vec![process(1, 0, "root")],
            threads: vec![
                thread(20, 1, Some("Worker")),
                thread(21, 1, Some("Worker")),
                thread(22, 1, None),
            ],
        });

        assert_eq!(
            render(&tree, None, RenderOptions::default()),
            "root-+-2*[{Worker}]\n     `-{root}\n"
        );
        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    numeric_sort: true,
                    ..RenderOptions::default()
                }
            ),
            "root-+-2*[{Worker}]\n     `-{root}\n"
        );
        assert_eq!(
            render(
                &tree,
                None,
                RenderOptions {
                    compact: false,
                    ..RenderOptions::default()
                }
            ),
            "root-+-{Worker}\n     |-{Worker}\n     `-{root}\n"
        );
    }

    #[test]
    fn duplicate_names_remain_distinguishable_by_pid_in_legacy_render() {
        let tree = ProcessTree::from_processes([process(1, 0, "worker"), process(2, 1, "worker")]);

        assert_eq!(
            tree.render(None, GlyphSet::Ascii).unwrap(),
            "worker(1)---worker(2)\n"
        );
    }

    #[test]
    fn thread_owner_missing_is_not_attached_to_a_process() {
        let tree = ProcessTree::from_snapshot(SystemSnapshot {
            processes: vec![process(1, 0, "root")],
            threads: vec![thread(20, 99, Some("orphan"))],
        });

        assert!(tree.threads_of(1).is_empty());
    }

    #[test]
    fn unknown_pid_is_an_error() {
        let tree = ProcessTree::from_processes([process(1, 0, "root")]);
        assert_eq!(
            tree.render_with_options(Some(99), GlyphSet::Ascii, &RenderOptions::default()),
            Err(RenderError::ProcessNotFound(99))
        );
    }
}
