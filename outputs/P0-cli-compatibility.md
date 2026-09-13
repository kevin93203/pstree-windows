# P0：Linux `pstree` CLI 相容性基線

## 目標

建立 `pstree-windows` 的命令列相容性契約，先固定 Linux `psmisc` `pstree` 的旗標名稱、錯誤行為與輸出語意，再讓後續 P1～P3 依同一份契約擴充。

P0 不負責一次完成所有 Linux 功能；它負責讓 parser、help、version、相容性矩陣與測試架構先穩定，避免每個後續功能各自定義一套 CLI 規則。

Linux 官方手冊列出的主要介面為 `pstree [option ...] [pid, user]`，包含 `-p/--show-pids`、`-n/--numeric-sort`、`-c/--compact-not`、`-s/--show-parents`、`-t/--thread-names` 與 `-T/--hide-threads` 等選項。[官方手冊](https://man7.org/linux/man-pages/man1/pstree.1.html)

## 目前基線

目前程式已具備：

- `--ascii`、`--unicode`、`--help`、`--version`。
- 一個可選 PID。
- `args_os` parser，不依賴 `clap`。
- Toolhelp process snapshot。
- 預設顯示 `name(PID)`，子程序依 PID 排序。
- 不做相同子樹壓縮、不顯示執行緒、不讀取命令列或路徑。

以下行為與 Linux `pstree` 不同，必須在相容性工作中明確處理：

| 項目 | 目前行為 | Linux 目標行為 |
| --- | --- | --- |
| PID 顯示 | 預設顯示 | `-p` 才顯示，且會停用程序壓縮 |
| 排序 | 預設依 PID | 預設依名稱；`-n` 依 PID |
| 子樹壓縮 | 永遠關閉 | 預設開啟；`-c` 關閉 |
| `-h` | help 別名 | highlight current process and ancestors |
| 執行緒 | 永遠隱藏 | 預設顯示；`-T` 隱藏 |
| positional argument | 僅支援 PID | PID 或 user name |

## 相容性決策

### 建議採用的方案

1. 以 Linux 旗標為主要介面：`-A/-U/-p/-n/-c/-s/-t/-T/-V` 與對應長選項均使用相同名稱。
2. `-h` 改回 Linux 的 highlight 語意；`--help` 保留為 help，另以 `-?` 作為短 help 別名。這會是相對目前 v1 的 breaking change，但能避免同一個短旗標有不同含義。
3. `-p` 改為控制 PID 是否顯示，而不是永遠顯示 PID。這是 Linux 相容性最容易被腳本與使用者察覺的輸出差異。
4. 尚未能提供等價語意的旗標，不接受 silent no-op；parser 應回報「Windows 尚未支援」並使用 exit code `2`。
5. 舊版 `--ascii`、`--unicode`、`--help`、`--version` 長選項繼續保留，讓既有使用方式不必立即修改。

### 需要保留在實作前的決策點

這些決策不阻塞本次規劃，但在合併 P0 實作前應確認：

| 決策 | 建議 | 若選另一方案的影響 |
| --- | --- | --- |
| `-h` 是否改為 highlight | 是 | 需保留非 Linux 語意，文件與 help 會不一致 |
| 是否採 Linux 的 PID 預設隱藏 | 是 | 輸出相容性較低，但可保留目前 v1 直覺 |
| 未支援旗標是否先接受 | 否，直接報錯 | 接受 no-op 會造成使用者誤以為功能已啟用 |
| 是否支援短旗標合併，例如 `-Apc` | 是 | parser 較簡單，但與常見 Linux 使用方式不一致 |
| user positional argument | P3 再做 | P0～P2 先只接受 PID，避免現在引入 token/使用者查詢 |

## 旗標分層

P0 先建立 parser enum 與相容性表，不必在 P0 立即實作每個功能的後端：

| 旗標 | P0 parser | 實際功能計畫 |
| --- | --- | --- |
| `-A/--ascii` | 實作 | P0/P1 |
| `-U/--unicode` | 實作 | P0/P1 |
| `-V/--version` | 實作 | P0 |
| `--help`、`-?` | 實作 | P0 |
| `-h/--highlight-all` | 解析選項 | P2 |
| `-p/--show-pids` | 解析選項 | P1 |
| `-n/--numeric-sort` | 解析選項 | P1 |
| `-c/--compact-not` | 解析選項 | P1 |
| `-s/--show-parents` | 解析選項 | P1 |
| `-t/--thread-names` | 解析選項 | P1 |
| `-T/--hide-threads` | 解析選項 | P1 |
| `-a/--arguments` | 暫時回報未支援，或登記 feature gate | P2 |
| `-l/--long` | 暫時回報未支援，或登記 feature gate | P2 |
| `-P/--show-paths` | 暫時回報未支援，或登記 feature gate | P2 |
| `-C/--color=TYPE`、`-H/--highlight-pid=PID` | 暫時回報未支援 | P2 |
| `-g/-k/-N/-S/-u/-Z` | 明確回報 Windows 不等價或未支援 | P3 評估 |

## 實作工作項目

### 1. 重整選項資料型別

將目前 `GlyphMode` 與 `Options` 擴充成單一 CLI 設定型別，至少包含：

```text
CliOptions {
    selected_pid: Option<u32>,
    show_pids: bool,
    numeric_sort: bool,
    compact: bool,
    show_parents: bool,
    show_threads: bool,
    thread_names: bool,
    glyph_mode: Auto | Ascii | Unicode | Vt100,
    ...future fields...
}
```

布林欄位要在 parser 階段完成衝突檢查，例如 `-A` 與 `-U`、`-t` 與 `-T`、`-h` 與 `-H` 的互斥關係。

### 2. 更新 parser

- 繼續使用 `std::env::args_os`。
- 支援單字元旗標與長旗標。
- 支援合併短旗標，例如 `-pn`；需要參數的選項不可與其他短旗標混在同一 token 中，除非 parser 明確處理。
- PID 僅接受十進位正整數，拒絕 `0`、負號、溢位與非數字。
- `--` 後的值視為 positional argument。
- 暫不接受第二個 positional argument，user name 的支援放到 P3。
- 未知或尚未支援的旗標輸出 stderr，exit code `2`。

### 3. 固定 help 與 version

Help 必須列出：

- synopsis：`pstree [OPTION]... [PID]`
- 已實作旗標。
- Windows 限制與目前未支援旗標。
- PID 篩選與輸出模式範例。

Version 至少輸出：

```text
pstree-windows 0.1.0
```

### 4. 建立相容性測試表

使用 parser unit tests 覆蓋：

- 每個短/長旗標的解析。
- 短旗標合併。
- `-A`/`-U`、`-t`/`-T`、`-h`/`-H` 衝突。
- PID 解析與 `--`。
- 重複 PID、未知旗標、未支援旗標。
- `--help`、`-?`、`--version` 不進入 snapshot 流程。
- exit code `0/1/2` 的契約。

## Definition of Done

- `cargo fmt --check` 通過。
- `cargo clippy --all-targets -- -D warnings` 通過。
- `cargo test` 通過。
- help 輸出與 parser 的旗標矩陣一致。
- README 的 CLI 範例不再使用與 Linux 目標衝突的 `-h` 語意。
- 所有未支援旗標都明確失敗，不會無聲忽略。

## 不在 P0

- Toolhelp thread snapshot。
- 命令列、完整路徑、token、session 或 security context。
- 子樹壓縮演算法。
- 顏色、highlight 的實際 console 寫入。
- user positional argument。

