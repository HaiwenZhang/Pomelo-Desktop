<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>逐層看清每一處連接。</strong><br>
  基於 GPUI 和 GPUI Kit 的本機唯讀 PCB 檢視器。
</p>

<p align="center"><a href="README.md">English</a> · <a href="README_zh-CN.md">简体中文</a> · 繁體中文 · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D4" alt="Windows, macOS, Linux">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11%20%7C%20Metal%20%7C%20wgpu-58752c" alt="D3D11, Metal, wgpu">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

## 更清晰地檢視電路板

- **設計檔案留在本機。** 在電腦上匯入與檢視電路板，無須上傳；檢視操作不會修改原始檔案。
- **原生 GPU 繪製。** 使用 D3D11/HLSL、Metal/MSL 與 wgpu/WGSL 顯示走線、圓弧、焊盤、導通孔、銅箔及孔洞、繪圖和 MSDF 板上文字。
- **追蹤連接。** 搜尋網路與元件，定位搜尋結果，並檢查選取物件的屬性。
- **控制顯示。** 管理圖層，依圖層或網路著色，調整銅箔透明度、焊盤填滿和標籤顯示。
- **同時檢視多塊板。** 支援多文件、最近開啟的檔案與檢視偏好儲存，方便繼續檢視設計。
- **選擇熟悉的語言。** 提供英文、簡體中文、繁體中文、日文、韓文介面，以及淺色和深色主題。

> **開發中：** 相容性、顯示精度、效能與各平台穩定性仍需驗證。本專案用於唯讀檢視，不編輯電路板或執行 DRC。見[開發說明](docs/development-progress.md)。

## 支援的格式

支援 `.brd`、`.PcbDoc`、ODB++ 封存檔（`.tgz`、`.tar`、`.tar.gz`）、`.pcb`、`.kicad_pcb` 和 `edb.def`。匯入範圍取決於檔案版本與已儲存資料，未支援的內容透過診斷提示。

所有格式解析均在 `crates/pomelo-import` 中使用 Rust 實作，無需 JavaScript 引擎或 Node.js。

## 快速開始

需要 Git 與儲存庫固定的 Rust **1.98.1**；依賴版本以 `Cargo.lock` 為準。

| 平台 | 建置與執行條件 |
| --- | --- |
| Windows x64 | MSVC C++ 建置工具、Windows SDK、支援 Direct3D 11 的顯示卡與驅動程式 |
| macOS | 支援 Metal 的 Mac、Xcode Command Line Tools |
| Linux | fontconfig、FreeType、xkbcommon、Wayland/X11、ALSA、OpenSSL 開發套件與支援 Vulkan 的驅動程式 |

下方建置命令適用於三個平台。macOS 的應用程式快捷鍵使用 Cmd 取代 Ctrl。

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
cargo run -p pomelo --locked
```

使用 **Ctrl+O**、拖放或啟動檔案路徑開啟電路板。在應用程式選單或工具列中選擇介面語言。

```powershell
cargo run -p pomelo --locked -- --locale zh-TW --encoding windows-1252 "example.brd"
```

`--locale` 支援 `en`、`zh-CN`、`zh-TW`、`ja`、`ko`，僅對本次啟動生效，不覆寫已儲存的語言偏好。`--encoding` 支援 `auto`、`utf-8`、`gbk`、`shift_jis`、`big5`、`windows-1252`，預設為 `auto`；自動識別不準確時可明確指定編碼。介面語言與來源編碼獨立，編碼選項適用於本次程序開啟的檔案。`--` 後的參數視為檔案路徑。

## 快捷操作

| 操作 | 按鍵 |
| --- | --- |
| 開啟檔案 | `Ctrl+O` |
| 關閉文件 | `Ctrl+W` |
| 重新載入文件 | `Ctrl+R` |
| 下一個／上一個文件 | `Ctrl+Tab / Ctrl+Shift+Tab` |
| 縮放至電路板全貌 | `F2` |
| 聚焦搜尋 | `Ctrl+K` |
| 設定 | `Ctrl+,` |
| 切換左側／右側面板 | `Ctrl+B / Ctrl+Shift+B` |

## 建置與開發

依賴版本與來源以 `Cargo.toml` 和 `Cargo.lock` 為準。見 [GPUI 依賴說明](docs/gpui-dependencies.md)及[GPU 後端](docs/native-gpu-backends.md)。

```powershell
cargo build -p pomelo --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p pomelo-import --bin pcb_inspect --locked -- --help
```

### Release 打包

| 平台／架構 | 產物 | 指令 |
| --- | --- | --- |
| Windows x64 | `.exe` | `scripts\build-windows.bat` |
| macOS arm64 / x64 | `.dmg`, `.app.zip` | `scripts/build-macos.sh` |
| Ubuntu 24.04+ amd64 / arm64 | `.deb`, `.tar.gz` | `scripts/build-ubuntu.sh` |

需要 Rust 與各平台原生建置相依套件；macOS 另需 ImageMagick，Windows 另需 Inno Setup 6，Ubuntu 另需 `dpkg-dev` 和 `desktop-file-utils`。產物輸出至 `dist`。

Shell 腳本支援 `--offline`、`--toolchain NAME`、`--output-dir PATH`，以及 `--binary PATH` 打包既有程式。正式建置使用 `cargo build --release --locked`，不需要 Python。

Ubuntu Release 固定在 24.04 建置，維持 24.04 的 ABI 基線，並於 CI 檢查 26.04 安裝與執行。若本機在更新的 Ubuntu 建置，可能引入更新的函式庫版本需求。

macOS 預設臨時簽名，最低版本為 macOS 14.0。可設定 `MACOS_SIGNING_IDENTITY` 使用鑰匙圈中的簽名身分，尚未設定 Apple 公證。

[Release 工作流程](.github/workflows/release.yml)：推送 `vX.Y.Z` 標籤後建置全部平台，通過驗證後發佈安裝包與 `SHA256SUMS`；標籤必須與 `Cargo.toml` 的 workspace 版本一致。手動觸發僅上傳 Actions 產物。

## 專案結構與文件

| 目錄 | 用途 |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | 電路板模型、搜尋、選取與國際化 |
| [crates/pomelo-import](crates/pomelo-import) | 檔案匯入與診斷 |
| [crates/pomelo-render](crates/pomelo-render) | 場景準備與 D3D11、Metal、wgpu 繪製 |
| [crates/pomelo](crates/pomelo) | GPUI 桌面應用程式與文件管理 |
| [locales](locales) | 五種語言的介面資源 |
| [docs](docs) | 通用開發說明 |

- [文件](docs/README.md)
- [開發進度與驗證範圍](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU](docs/native-gpu-backends.md)

## 開發計畫

以下為待開發或待補齊的能力，不代表已交付功能。

| 階段 | 計畫內容 |
| --- | --- |
| 日常審閱 | 兩點與折線測量、物件淨距、任意多選、持久多組高亮 |
| 檢視與輸出 | 命名視圖、導覽歷史、自訂顯示、截圖與輕量本機匯出 |
| 工程資訊 | 屬性查詢、逐層焊盤結構、物理疊層、網路分組、連接拓撲與指定路徑長度 |
| 持續改善 | 檔案相容性、大型場景效能、跨平台穩定性、鍵盤操作與五語介面 |

詳細優先順序與功能邊界見[審閱能力規劃](docs/viewer-review-capabilities-plan.md)，實作與驗收要求見[開發設計](docs/viewer-review-development-design.md)。

功能圍繞唯讀檢視與審閱，不編輯來源設計或執行 DRC。

## 參與貢獻

歡迎改善檔案相容性、繪製精度、效能、翻譯與文件。[回報問題](https://github.com/HaiwenZhang/Pomelo-Desktop/issues)時，請附上來源工具與檔案版本、作業系統和 GPU 資訊、重現步驟及匯入診斷。小型範例或對照截圖有助於定位問題；分享前請移除機密設計資料。解析或幾何變更應提供針對性的回歸測試，視覺變更應附截圖，並保持五種語言的 README 同步。

## 授權

專案程式碼採用 [MIT 授權](LICENSE)。內附字型保留各自的授權，詳見[思源黑體](assets/fonts/source-han-sans/README.md)。

## 致謝

感謝 [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) 和 [GPUI Kit](https://github.com/longbridge/gpui-kit) 專案，為 Pomelo Desktop 提供 GPU 加速的 UI 框架和 UI 元件。
