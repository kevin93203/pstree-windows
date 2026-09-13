# P1：程序、執行緒與樹狀語意

## 目標

讓 `pstree-windows` 完成 Linux `pstree` 最核心的樹狀語意：程序父子關係、執行緒顯示、PID 篩選、排序、子樹壓縮，以及 `-p/-n/-c/-s/-t/-T` 的可用行為。

Linux `pstree` 會把執行緒放在所屬程序下，預設顯示執行緒；`-T` 只顯示程序，`-t` 在可取得時顯示完整執行緒名稱。[官方手冊](https://man7.org/linux/man-pages/man1/pstree.1.html)

## 依賴與邊界

P1 依賴 P0 的 parser 契約，但仍只做單次 snapshot：

- 不做 watch、TUI、程序控制或重新查詢。
- 不因單一程序/執行緒權限不足而讓整個 snapshot 失敗。
- 程序或執行緒在快照前後消失時，接受 best-effort 結果。
- 不把 Windows thread 假裝成真正的 process child；它們是 renderer 的 pseudo-child。

## 資料模型

保留目前純程序圖的簡單結構，新增獨立執行緒資料，不引入泛用 `Node` trait 或多層抽象：

```text
ProcessInfo {
    pid: u32,
    parent_pid: u32,
    name: String,
}

ThreadInfo {
    tid: u32,
    owner_pid: u32,
    name: Option<String>,
}

SystemSnapshot {
    processes: Vec<ProcessInfo>,
    threads: Vec<ThreadInfo>,
}
```

`ProcessTree` 繼續負責 process-to-process 關係，另以 `BTreeMap<u32, Vec<ThreadInfo>>` 依 owner PID 索引執行緒。這樣可避免把 TID 混入 PID key，並保留既有程序樹 unit tests 的簡潔性。

## Windows 收集流程

### 1. 建立一次 Toolhelp snapshot

優先以同一個 snapshot 同時取得：

```text
TH32CS_SNAPPROCESS | TH32CS_SNAPTHREAD
```

使用目前的 RAII handle wrapper，確保 `CloseHandle` 在所有錯誤路徑執行。Toolhelp snapshot API 能包含 processes 與 threads；`TH32CS_SNAPTHREAD` 產生的是全系統執行緒清單，不是只針對某一個 PID。[Microsoft Toolhelp 文件](https://learn.microsoft.com/en-us/windows/win32/toolhelp/snapshots-of-the-system)

### 2. Enumerate processes

沿用 `Process32FirstW` / `Process32NextW` 與目前的 UTF-16 lossily conversion，產生 `ProcessInfo`。

### 3. Enumerate threads

使用 `Thread32First` / `Thread32Next` 產生 `ThreadInfo`：

- `th32ThreadID` → `tid`。
- `th32OwnerProcessID` → `owner_pid`。
- 若 owner PID 不在 process snapshot，保留或丟棄都可以，但 renderer 不得把它輸出成孤立 root；建議丟棄。
- 依 TID 遞增排序，確保輸出穩定。

`THREADENTRY32.th32OwnerProcessID` 是把執行緒掛到程序下的關鍵欄位。[Microsoft `Thread32First` 文件](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-thread32first)

### 4. 讀取執行緒名稱

只有使用者要求 `-t` 時才開啟 thread description 查詢，避免預設輸出多出不必要的 handle 操作：

- `OpenThread(THREAD_QUERY_LIMITED_INFORMATION, ...)`。
- `GetThreadDescription`。
- 成功時儲存 `String`。
- 沒有名稱、API 不存在、權限不足、thread 已結束時，使用 `None`。
- 由 API 配置的字串依文件要求釋放。

`GetThreadDescription` 需要 `THREAD_QUERY_LIMITED_INFORMATION`，部分 thread 不一定能查詢；Windows 10 1607/Server 2016 也需要考慮 runtime dynamic linking。[Microsoft `GetThreadDescription` 文件](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreaddescription)

## 樹狀建構規則

### Process-to-process

沿用目前規則：

- PID 建立 index。
- parent PID 存在且不等於自身時，加入 parent 的 children。
- parent 不存在或 self-parent 時，視為 root。
- children 預設依名稱排序；同名時依 PID 遞增作為 tie-breaker。
- `-n` 改為只依 PID 遞增排序。
- 使用 visited guard 防止異常父子資料造成無限遞迴。

### 指定 PID

- 無 PID：輸出所有 roots。
- 有 PID：只輸出指定程序與後代。
- PID 不存在：stderr，exit code `1`。
- 有 `-s/--show-parents`：先輸出從 root 到指定 PID 的祖先鏈，再在指定 PID 展開後代。
- 若祖先資料在 snapshot 中缺失，從可取得的最接近節點開始，不重新查詢。

### Thread pseudo-child

每個 process node 的輸出順序建議固定為：

1. 目前程序名稱。
2. 該程序的執行緒，依 TID 遞增。
3. 該程序的子程序，依目前排序模式。

建議輸出：

```text
app.exe(1200)
├─{MainThread}[TID=2400]
├─{Worker}[TID=2401]
└─child.exe(2500)
```

相容性注意：Linux `-p` 是顯示 process PID；Windows 沒有 Linux 的 `/proc` thread 命名與格式。建議 Windows 在 `-p` 開啟時顯示 TID，使用固定的 `[TID=n]` 標記，不把 TID 稱為 PID。

## 旗標行為

### `-p/--show-pids`

- 預設：只輸出程序名稱與執行緒名稱。
- `-p`：程序輸出 `name(PID)`。
- thread pseudo-child 同時輸出 `[TID=n]`，避免同名 thread 無法辨識。
- 如啟用 `-p`，停用 process subtree compaction，符合 Linux 手冊對 `-p` 的描述。

### `-T/--hide-threads`

- 隱藏所有 thread pseudo-child。
- 不影響 process snapshot 的 parent/child 關係。
- `-T` 與 `-t` 互斥；若兩者同時出現，exit code `2`。

### `-t/--thread-names`

- 顯示完整 thread description；若不可用，使用穩定 fallback。
- 不改變是否顯示 thread；是否顯示由預設模式與 `-T` 控制。
- 對沒有名稱的 thread 不顯示空大括號，使用 `{TID=n}`。

### `-c/--compact-not`

實作最小可預期的相同子樹壓縮：

- 只壓縮同一父程序下、名稱與完整後代結構都相同的 process branches。
- thread list 必須視為 branch signature 的一部分，否則開啟 thread 顯示時會錯誤合併不同節點。
- 顯示為 `N*[name]`，並保留 PID/TID 模式下的可辨識性規則。
- `-c` 停用 process branch compaction，但不必停用 thread name grouping，符合 Linux 的「process compaction」與「thread grouping」分離概念。
- `-p` 隱含停用 process compaction。

若這個演算法讓 P1 過度膨脹，第一個可交付版本可以先令 `-c` 有正確 parser 與明確錯誤訊息，將壓縮延後到 P2；不可把 `-c` 當成 silent no-op。

## Renderer 重構

- 將「目前節點是否為最後一個 child」與「下一層 prefix」整理成共用函式。
- Process child 與 thread pseudo-child 共用 connector rendering，但 thread label 使用 `{}`。
- ASCII：`|-`、`` `- ``、`| `。
- Unicode：`├─`、`└─`、`│ `。
- root 選擇、visited guard、broken pipe 行為維持現有契約。
- 不在這一階段加入顏色、寬度截斷或 terminal UI。

## 測試

### Unit tests

- 無執行緒、單一執行緒、多執行緒。
- thread 依 TID 排序。
- `-T` 完全隱藏 thread。
- `-t` 有名稱、無名稱、查詢失敗 fallback。
- 同名 process 與同名 thread 仍可由 PID/TID 區分。
- process 預設名稱排序與 `-n` PID 排序。
- `-p` 顯示 process PID/TID，並停用壓縮。
- `-s` 顯示祖先鏈。
- 指定 PID 只包含目標、祖先（若 `-s`）與後代。
- process cycle、thread owner 缺失不造成無限遞迴。

### Windows integration tests

- snapshot 能列出目前測試程序。
- snapshot 能列出目前程序所擁有的 thread。
- 啟動 child process 後驗證 process parent PID。
- 建立測試 thread 並嘗試設定 description，驗證 `-t` 的 best-effort 結果。
- 執行目前測試程序 PID，驗證 `-p`、`-T`、`-t` 與 `-s`。
- thread 在查詢期間結束時，命令仍能完成。

## Definition of Done

- Windows 10/11 x64 可使用 Toolhelp snapshot 完成 process + thread 單次快照。
- 預設顯示 threads，`-T` 可隱藏，`-t` 可顯示可取得的完整名稱。
- `-p/-n/-c/-s` 的實際行為與 help 描述一致。
- PID 過濾、排序、cycle guard 與 broken pipe 均有測試。
- 不需要系統管理員權限；個別權限不足只造成節點名稱 fallback 或略過。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 與 Windows release build 通過。

## 不在 P1

- 命令列 arguments、完整路徑、user/token、security context。
- color/highlight。
- Linux namespace、PGID、kernel thread 的假相容性。

