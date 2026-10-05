<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>逐層看清每一處連接。</strong><br>
  基於 GPUI 和 GPUI Kit 的 PCB 檢視器，支援 Cadence Allegro (.brd)、Altium Designer、ODB++、PADS、Ansys HFSS 3D Layout(edb.def) 和 KiCad。
</p>

<p align="center"><a href="README.md">English</a> · <a href="README_zh-CN.md">简体中文</a> · 繁體中文 · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center"><a href="https://github.com/HaiwenZhang/Pomelo-Desktop/releases">下載發行版本</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D4" alt="Windows, macOS, Linux">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11%20%7C%20Metal%20%7C%20wgpu-58752c" alt="D3D11, Metal, wgpu">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

![電路板工作區（英文介面）](images/pcb-example-en.png)

## 更清晰地檢視電路板

- **設計檔案留在本機。** 在電腦上匯入與檢視電路板，無須上傳；檢視操作不會修改原始檔案。
- **原生 GPU 繪製。** 使用 D3D11/HLSL、Metal/MSL 與 wgpu/WGSL 顯示走線、圓弧、焊盤、導通孔、銅箔及孔洞、繪圖和 MSDF 板上文字。
- **追蹤連接。** 搜尋網路與元件，定位搜尋結果，並檢查選取物件的屬性。
- **控制顯示。** 管理圖層，依圖層或網路著色，調整銅箔透明度、焊盤填滿和標籤顯示。
- **同時檢視多塊板。** 支援多文件、最近開啟的檔案與檢視偏好儲存，方便繼續檢視設計。
- **選擇熟悉的語言。** 提供英文、簡體中文、繁體中文、日文、韓文介面，以及淺色和深色主題。

> **開發中：** 完整 BRD MVP 驗收尚未完成。顯示精度、大型電路板效能、DPI／輸入法與長時間執行穩定性仍需更廣泛驗證。本專案是唯讀檢視器，不編輯電路板，也不執行 DRC。驗證範圍請見[開發進度](docs/development-progress.md)。

## 支援的格式

目前支援 **Cadence Allegro 二進位 `.brd` 檔案**。這不代表支援其他 EDA 工具使用的同名副檔名。匯入範圍與顯示精度取決於檔案版本及內容。Web 版支援的其他格式尚未整合至本桌面專案。

## 快速開始

macOS 需要支援 Metal 的 Mac 與 Xcode Command Line Tools。Linux 需要 fontconfig、FreeType、xkbcommon、Wayland/X11、ALSA、OpenSSL 開發套件，以及支援 Vulkan 的顯示卡驅動程式。下方建置命令適用於三個平台；macOS 應用程式快捷鍵使用 Cmd 取代 Ctrl。

需要 **Windows x64**、支援 **Direct3D 11** 的顯示卡與驅動程式、**Git**，以及 Rust、**MSVC C++ 建置工具和 Windows SDK**。儲存庫透過 `rust-toolchain.toml` 固定 Rust **1.98.1**，相依版本以 `Cargo.lock` 為準。

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
cargo run -p pomelo --locked
```

使用 **Ctrl+O**、拖放或啟動檔案路徑開啟電路板。在應用程式選單或工具列中選擇介面語言。

![歡迎畫面（英文介面）](images/welcome-en.png)

```powershell
cargo run -p pomelo --locked -- --locale zh-TW --encoding windows-1252 "C:\boards\example.brd"
```

`--locale` 支援 `en`、`zh-CN`、`zh-TW`、`ja`、`ko`，僅對本次啟動生效，不覆寫已儲存的語言偏好。`--encoding` 支援 `utf-8`、`gbk`、`shift_jis`、`big5`、`windows-1252`，預設為嚴格 UTF-8；所選編碼適用於本次程序開啟的檔案。介面語言與原始檔案編碼互相獨立。`--` 後的參數全部視為檔案路徑。

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

GPUI 暫時依賴 [HaiwenZhang/zed 的 gpui-pre-0.3.7-native-gpu 分支](https://github.com/HaiwenZhang/zed/tree/gpui-pre-0.3.7-native-gpu)，由 Cargo.lock 鎖定到 `8c92bda2dc9d718cd520d37fda20bca00fa3e274`。Cargo 直接取得此 fork，無須準備本機修補。薄包名適配層讓 GPUI Kit 0.7.0 使用同一套 GPUI 型別。詳見 [GPUI 依賴說明](docs/gpui-dependencies.md)。一般桌面執行不需要 Node.js 或 Web 儲存庫。Windows 使用 D3D11，macOS 使用 Metal，Linux 使用 wgpu；共用場景與批次邏輯，各平台獨立維護著色器。 GPUI Kit 直接使用 registry 原版套件，一般 Cargo 和 rust-analyzer 無須修補準備。

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
| Windows ARM64 | `.exe` | `scripts\build-windows.bat --arch arm64` |
| macOS arm64 / x64 | `.dmg`, `.app.zip` | `scripts/build-macos.sh` |
| Ubuntu 24.04+ amd64 / arm64 | `.deb`, `.tar.gz` | `scripts/build-ubuntu.sh` |

需要 Rust 與各平台原生建置相依套件；macOS 另需 ImageMagick，Windows 另需 Inno Setup 6，Ubuntu 另需 `dpkg-dev` 和 `desktop-file-utils`。產物輸出至 `dist`。

Shell 腳本支援 `--offline`、`--toolchain NAME`、`--output-dir PATH`，以及 `--binary PATH` 打包既有程式。正式建置使用 `cargo build --release --locked`，不需要 Python。

Ubuntu Release 固定在 24.04 建置，維持 24.04 的 ABI 基線，並於 CI 檢查 26.04 安裝與執行。若本機在更新的 Ubuntu 建置，可能引入更新的函式庫版本需求。

macOS 預設臨時簽名，最低版本為 macOS 14.0。可設定 `MACOS_SIGNING_IDENTITY` 使用鑰匙圈中的簽名身分，尚未設定 Apple 公證。

[Release 工作流程](.github/workflows/release.yml)：推送 `vX.Y.Z` 標籤後建置全部平台，通過驗證後發佈安裝包與 `SHA256SUMS`；標籤必須與 `Cargo.toml` 的 workspace 版本一致。手動觸發僅上傳 Actions 產物。


| 目錄 | 用途 |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | 電路板模型、搜尋、選取與國際化 |
| [crates/pomelo-import](crates/pomelo-import) | Allegro 解析與匯入診斷 |
| [crates/pomelo-render](crates/pomelo-render) | 場景準備與 D3D11、Metal、wgpu 繪製 |
| [crates/pomelo](crates/pomelo) | GPUI 桌面應用程式與文件管理 |
| [locales](locales) | 五種語言的介面資源 |
| [docs](docs) | 研發計畫與驗證證據 |

- [研發計畫](docs/native-pcb-viewer-development-plan.md)
- [開發進度與驗證範圍](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU / ADR](docs/adr/0001-gpui-pre-0.3.7-native-gpuing.md)

## 參與貢獻

歡迎改善 Allegro 相容性、繪製精度、效能、翻譯與文件。[回報問題](https://github.com/HaiwenZhang/Pomelo-Desktop/issues)時，請附上來源工具與檔案版本、作業系統和 GPU 資訊、重現步驟及匯入診斷。小型範例或對照截圖有助於定位問題；分享前請移除機密設計資料。解析或幾何變更應提供針對性的回歸測試，視覺變更應附截圖，並保持五種語言的 README 同步。

## 授權

專案程式碼採用 [MIT 授權](LICENSE)。內附字型保留各自的授權，詳見[思源黑體](assets/fonts/source-han-sans/README.md)。

## 致謝

感謝 [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) 和 [GPUI Kit](https://github.com/longbridge/gpui-kit) 專案，為 Pomelo Desktop 提供 GPU 加速的 UI 框架和 UI 元件。
