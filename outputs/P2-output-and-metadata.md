# P2：輸出、程序 metadata 與互動式顯示

## 目標

在 P1 已有穩定 process/thread tree 後，補上 Linux `pstree` 的資訊與顯示功能：命令列、完整路徑、長行、顏色、highlight，以及更完整的 output-mode 行為。

P2 的原則是 best effort：Windows 上不能讀取某個程序的資訊時，只影響該節點的附加欄位，不讓整棵樹失敗。程序名稱與 PID 仍以 Toolhelp snapshot 的結果為準。

Linux 手冊將 `-a` 定義為 command-line arguments、`-l` 為 long lines、`-P` 為 full paths、`-C` 為依程序年齡著色，並以 `-h/-H` 提供 highlight。[官方手冊](https://man7.org/linux/man-pages/man1/pstree.1.html)

## 功能優先順序

| 順序 | 旗標 | 建議 | 原因 |
| --- | --- | --- | --- |
| P2.1 | `-P/--show-paths` | 先做 | 有 documented Win32 API，風險最低 |
| P2.2 | `-l/--long` | 接著做 | 主要是 renderer/layout，與 metadata 耦合低 |
| P2.3 | `-h/-H` | 接著做 | 只影響互動式 console，redirected output 可安全停用 |
| P2.4 | `-C/--color=age` | 接著做 | Windows console/VT 模式有平台差異 |
| P2.5 | `-a/--arguments` | 最後做 | 遠端程序命令列沒有簡單、穩定、完全 documented 的單一 API |

## 共通資料模型

不要把附加資訊硬塞進 `ProcessInfo.name`。建議新增可選欄位：

```text
ProcessMetadata {
    image_path: Option<String>,
    command_line: Option<String>,
    start_time: Option<SystemTime>,
}
```

`ProcessInfo` 保持 snapshot 的基本資料；metadata 由 renderer 根據旗標按需查詢。這避免預設模式為每個 process 開啟 handle，也避免一個 metadata API 失敗而破壞樹狀結構。

## P2.1 `-P/--show-paths`

### 實作

對每個需要輸出的 process：

1. `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, ...)`。
2. `QueryFullProcessImageNameW` 讀取完整 executable path。
3. 關閉 handle。
4. 查詢失敗時回退至 snapshot 的 executable name。

`QueryFullProcessImageNameW` 是 documented Win32 API；它需要 process handle 權限，因此 system/protected process 可能失敗。路徑查詢必須是逐節點 best effort。[Microsoft `QueryFullProcessImageNameW` 文件](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew)

### 輸出規則

- 預設仍顯示 basename。
- `-P` 顯示完整 path。
- `-p` 若同時存在，格式建議為 `C:\\path\\app.exe(1234)`。
- path 含空白時不額外加入引號，避免 renderer 與 Linux 類似工具的文字輸出產生混合格式；README 提供解析注意事項。
- 若 path 讀取失敗，不輸出假路徑或問號以外的推測值。

### 測試

- 目前測試程序能取得非空 path，或得到可預期的 permission fallback。
- `-P` 不會改變 process tree 關係。
- protected/system process 不使整次命令失敗。

## P2.2 `-l/--long`

### 版面策略

P1 的輸出不截斷；P2 才引入 width policy：

- 互動式 console：取得 console width。
- redirected stdout：讀取 `COLUMNS`；無法取得時採 132 columns，與 Linux 手冊描述相近。
- `-l` 停用 line truncation。
- 未使用 `-l` 時，優先在不切斷 UTF-8/UTF-16 字元的前提下截斷。
- 截斷只影響顯示文字，不影響 PID、TID、tree prefix 或內部資料。

### 注意事項

- Unicode display width 不等於 Rust `str.len()`；至少要避免在 UTF-8 code point 中間切割。
- P2 不實作完整 East Asian Width 或 terminal layout library；若之後發現中文程序名稱造成寬度問題，再增加明確需求。
- ASCII 與 Unicode prefix 的寬度都視為 2 個 display columns。

### 測試

- ASCII、Unicode、中文與 emoji 名稱不產生 invalid UTF-8。
- 短行不被不必要截斷。
- `-l` 的長行與預設截斷行分別驗證。
- redirected output 不依賴 console API。

## P2.3 `-h/--highlight-all` 與 `-H/--highlight-pid=PID`

### 語意

- `-h` 標記目前 `pstree` 程序與其祖先；只在目前輸出的 subtree 中存在的節點才標記。
- `-H PID` 標記指定 process；目標不在 snapshot 或不在輸出 subtree 時依 Linux 風格處理為無可標記節點，必要時回報 exit code `1`。
- `-h` 與 `-H` 互斥。

### 輸出方式

- 互動式 Windows console：使用 `SetConsoleTextAttribute` 或已啟用的 VT ANSI sequence。
- redirected stdout：不寫入 ANSI escape，`-h` 視為 no-op；`-H` 是否失敗要與 help 明確一致。
- 不把 color escape 寫進 `WriteConsoleW` 的純文字模式。
- broken pipe 維持 exit code `0`。

### 目前程序 PID

`std::process::id()` 可取得目前 `pstree` 程序 PID。祖先鏈使用 P1 已收集的 parent map，不重新查詢。

### 測試

- `-h` 標記目前程序與祖先。
- `-H` 只標記指定 PID。
- redirect 不含 ANSI escape。
- non-interactive terminal 的錯誤行為穩定。

## P2.4 `-C/--color=TYPE`

### 第一階段範圍

只支援 Linux 手冊目前列出的 `age`：

- 新於 60 秒：green。
- 新於 1 小時：yellow。
- 其餘：red。

其他 TYPE 直接 exit code `2`，不可默認成 `age`。

### Windows 實作

- 需要 process creation/start time；Toolhelp process entry 不直接提供完整 start time。
- 以 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 搭配 `GetProcessTimes` 取得 creation time。
- 權限不足時使用 neutral color，不影響樹狀輸出。
- console 使用 Windows console attribute 或 VT；redirected output 預設不輸出色碼。
- reset color 必須使用 scope guard 或集中 renderer，避免 error path 留下錯誤 console 色彩。

### 測試

- age 分界值使用合成時間測試。
- 無 creation time 時輸出仍完整。
- color 不改變純文字模式內容。
- `--color=invalid` 使用 exit code `2`。

## P2.5 `-a/--arguments`

### 技術風險

Windows 沒有和 Linux `/proc/<pid>/cmdline` 完全對等、同樣簡單的 documented API。常見做法是透過 `NtQueryInformationProcess` 取得 PEB 位址，再使用 `ReadProcessMemory` 讀取 RTL process parameters；這依賴 undocumented/版本敏感的結構，且受 WOW64、cross-bitness、protected process 與 race condition 影響。

因此 P2 的最低可交付方案是：

- `-a` 真正嘗試查詢，不做 silent no-op。
- 權限或格式不支援時，把該 process 顯示成名稱並加上明確的 unavailable marker，或依 help 契約只跳過 arguments。
- 不將查不到 command line 當成整次 snapshot error。
- 不新增 WMI、PowerShell 或外部命令依賴。
- 先以 x64 process 查詢 x64 process；WOW64/cross-bitness 明確測試後再擴充。

### 需要決策的地方

| 選項 | 建議 | 取捨 |
| --- | --- | --- |
| P2 實作 PEB best effort | 建議 | 不需新依賴，但依 undocumented 結構 |
| 延後 `-a`，先只做 `-P/-l/-h/-C` | 更保守 | Linux 旗標完成度較低，但穩定性較高 |
| 改用 WMI/PowerShell | 不建議 | 外部依賴、速度與部署複雜度上升 |

## README 與相容性矩陣

P2 完成後 README 必須說明：

- `-P`、`-a` 的權限與 best-effort 限制。
- `-l` 的 width 來源與預設值。
- `-h/-H/-C` 僅在互動式 console 提供視覺效果。
- redirected output 不包含 color escape。
- 每個 Linux 旗標是 exact、Windows adaptation、還是 unsupported。

## Definition of Done

- `-P`、`-l`、`-h`、`-H`、`-C=age` 有實際可驗證的行為。
- `-a` 若納入，必須有成功、權限不足、cross-bitness 或 unavailable fallback 測試；否則應明確標記為 deferred，而非宣稱完成。
- interactive 與 redirected output 不互相污染。
- metadata 查詢失敗不會破壞 process/thread tree。
- 無新增外部 runtime dependency。
- 文件中的每個已支援旗標都有範例與限制說明。

