# P3：進階相容性邊界與發佈品質

## 目標

處理 Linux `pstree` 旗標中沒有 Windows 完全等價物的部分，並把可支援、只能近似、明確不支援的行為固定下來；最後完成文件、跨版本測試與 release 品質。

Linux 官方手冊目前還包含 PGID、kernel thread、namespace、UID transition、security context 與 VT100 等功能；這些功能依賴 Linux `/proc`、process group、namespace 或 security model，不能直接從 Windows process parent PID 推導。[官方手冊](https://man7.org/linux/man-pages/man1/pstree.1.html)

## 相容性分級

P3 不追求「每一個 Linux 旗標都做一個看起來相似的 Windows 功能」。建議採下列分級：

| 分級 | 定義 | CLI 行為 |
| --- | --- | --- |
| Exact | 名稱與主要語意都能在 Windows 重現 | 正常執行 |
| Adapted | 使用 Windows 等價概念，但輸出與 Linux 不完全相同 | 正常執行並在 help/README 說明 |
| Unsupported | 沒有可靠等價物或需要不可接受的 undocumented 行為 | exit code `2`，明確說明原因 |
| Deferred | 有可行方案，但尚未達到品質門檻 | exit code `2`，列出未來版本 |

禁止把 Unsupported 或 Deferred 做成 silent no-op。

## 旗標評估與建議

### `-g/--show-pgids`

Linux PGID 是 POSIX process group ID。Windows console process group、Job Object 與 Linux PGID 的生命週期及語意不同，不能直接以其中一個欄位代替。

建議：

- P3 先標示 Unsupported。
- 不把 Job Object ID 假稱為 PGID。
- 若日後要加入 Windows extension，使用新名稱，例如 `--show-job-ids`，不要改變 `-g` 的 Linux 語意。

### `-k/--kthreads`

Linux kernel threads 是 `/proc` 可見的特殊執行緒。一般 Windows user-mode Toolhelp snapshot 不提供可安全列印的對應 kernel-thread tree。

建議：Unsupported。不要用 system process、idle process 或 driver thread 假造結果。

### `-N/--ns-sort=TYPE` 與 `-S/--ns-changes`

Linux namespace type（ipc、mnt、net、pid、time、user、uts）沒有一對一 Windows API。Windows session、Job、silo/container 與 namespace 的關係也不能用單一欄位等價替換。

建議：

- P3 先列為 Unsupported。
- 不把 Windows Session ID 假稱為 namespace。
- 若未來支援 Windows container/silo，新增 Windows-specific flag，另訂資料模型與權限文件。

### `-u/--uid-changes`

Linux UID 與 Windows token user/SID 不是同一概念。

可行的 Adapted 方案：

- `OpenProcessToken`。
- `GetTokenInformation(TokenUser)` 取得 SID。
- 比較程序與 parent 的 SID。
- 在輸出中標記 identity transition，但文件中明確稱為 Windows user/SID transition。

建議：先列為 Deferred；若產品需要「誰啟動了程序」的診斷能力，再以 Windows adaptation 實作，不宣稱是 Linux UID 相容。

### `-Z/--security-context`

可用 Windows token 的 integrity level、elevation type 或 SID 做近似，但這不是 SELinux security context。

建議：

- 第一階段標為 Unsupported。
- 若要支援，另定輸出欄位名稱，例如 `Integrity=High`、`User=...`，並在 help 標示 Windows adaptation。
- 不輸出看似 SELinux label 的虛構字串。

### `-G/--vt100`

這是 P3 中最容易做的相容性項目：

- 互動式 console 可使用 VT/Unicode line drawing mode。
- legacy console 若未啟用 VT，回退到現有 `WriteConsoleW`。
- redirected output 可輸出固定 VT100/ASCII glyph，但不得與 `-A`、`-U` 衝突。
- `-G` 與 `-A`、`-U` 的互斥規則寫入 parser tests。

建議：列為 Adapted，而不是宣稱和 Linux terminal 初始化行為完全相同。

## Windows identity extension（可選）

如果產品目標是 Windows 診斷工具，而非只做 Linux clone，P3 可新增不佔用 Linux 旗標語意的選項：

```text
--show-user       顯示 process token user/SID
--show-session    顯示 Windows Session ID
--show-job        顯示 Job Object 關聯
```

這些選項必須：

- 明確使用 Windows 名稱。
- 仍以 best effort 查詢。
- 不改動既有 process/thread tree key。
- 不把 Windows 欄位混入 Linux `-g/-u/-N/-Z` 的輸出格式。

若沒有實際使用情境，遵守 YAGNI，維持 Unsupported，不新增這些 flags。

## P3 發佈品質

### 建置矩陣

最低要求：

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --target x86_64-pc-windows-msvc
```

若 CI/硬體允許，再加入：

- `aarch64-pc-windows-msvc` cross build。
- Windows 10 與 Windows 11 實機整合測試。
- legacy console、Windows Terminal、PowerShell redirect、`cmd.exe` redirect。

### Snapshot robustness

測試並記錄下列 race condition：

- process 在 process snapshot 後結束。
- thread 在 `Thread32First/Next` 後結束。
- `OpenProcess` / `OpenThread` 被拒絕。
- `GetThreadDescription` 回傳 unavailable。
- path/arguments metadata 查詢失敗。
- stdout 在輸出中途收到 broken pipe。

所有這些情況都必須有明確分類：可忽略節點、fallback 欄位、exit code `1`，或正常 broken pipe `0`。

### CLI regression tests

建立一份 table-driven 測試，至少驗證：

- 每個 exact flag 的短/長名稱。
- 每個 adapted flag 的輸出與 help 說明。
- 每個 unsupported/deferred flag 的 exit code `2` 與 stderr。
- `-h` 不再被誤認為 help。
- `--help` 不會觸發 Windows API。
- `-p`、`-T`、`-t` 與 PID selection 的組合。
- `-A`、`-U`、`-G` 的模式衝突。

### README / man-like help

P3 完成時 README 要包含：

- Linux `pstree` 相容性分級表。
- Windows-specific adaptation 清單。
- 權限需求與 protected process 限制。
- 輸出格式範例。
- console/redirected encoding 行為。
- exit code 契約。
- 明確列出不支援 `-g/-k/-N/-S/-u/-Z` 的原因。

不需要在 P3 建 installer、Windows service、telemetry 或 watch mode；目前需求已排除這些功能。

## 版本策略

若 P0/P1 採用 Linux 預設行為，會產生幾項輸出 breaking change：

- PID 不再預設顯示，改由 `-p` 開啟。
- 預設顯示執行緒，`-T` 才隱藏。
- 預設可能啟用相同子樹壓縮。
- `-h` 從 help 改為 highlight。

建議在 `0.x` 直接完成遷移，並在 README 的 changelog 明確列出；若已經有外部使用者依賴目前 v1 輸出，則先發一個過渡版本並提供相容模式，但不要讓相容模式永久成為第二套未維護的 CLI。

## 最終 Definition of Done

- 所有 Linux 旗標都有 Exact、Adapted、Unsupported 或 Deferred 分類。
- Unsupported/deferred 行為明確且可測試。
- 沒有以 Job、Session、SID 或 token 欄位冒充 PGID、namespace、UID 或 SELinux context。
- Windows 10/11 x64 release binary 可重現建置。
- console、PowerShell、`cmd.exe` 與 redirected output 都有驗證結果。
- README、help、測試與實作的旗標名稱完全一致。
- 發佈內容只有必要的 `.exe` 與文件，不加入 installer、service 或 telemetry。

