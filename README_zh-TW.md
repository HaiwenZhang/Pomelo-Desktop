<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>逐層看清每一處連接。</strong><br>
  使用 Rust 與 GPUI Kit 建構的原生 PCB 檢視器。在 Windows 上透過本機檔案處理與 GPU 繪製，探索 Cadence Allegro 電路板。
</p>

<p align="center"><a href="README.md">English</a> · <a href="README_zh-CN.md">简体中文</a> · 繁體中文 · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows-0078D4" alt="Windows">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11-58752c" alt="D3D11">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

![電路板工作區（英文介面）](images/pcb-example-en.png)

## 更清晰地檢視電路板

- **設計檔案留在本機。** 在電腦上匯入與檢視電路板，無須上傳；檢視操作不會修改原始檔案。
- **原生 GPU 繪製。** 使用 D3D11 與 HLSL 顯示走線、圓弧、焊盤、導通孔、銅箔及孔洞、繪圖和 MSDF 板上文字。
- **追蹤連接。** 搜尋網路與元件，定位搜尋結果，並檢查選取物件的屬性。
- **控制顯示。** 管理圖層，依圖層或網路著色，調整銅箔透明度、焊盤填滿和標籤顯示。
- **同時檢視多塊板。** 支援多文件、最近開啟的檔案與檢視偏好儲存，方便繼續檢視設計。
- **選擇熟悉的語言。** 提供英文、簡體中文、繁體中文、日文、韓文介面，以及淺色和深色主題。

> **開發中：** 完整 BRD MVP 驗收尚未完成。顯示精度、大型電路板效能、DPI／輸入法與長時間執行穩定性仍需更廣泛驗證。本專案是唯讀檢視器，不編輯電路板，也不執行 DRC。驗證範圍請見[開發進度](docs/development-progress.md)。

## 支援的格式

目前支援 **Cadence Allegro 二進位 `.brd` 檔案**。這不代表支援其他 EDA 工具使用的同名副檔名。匯入範圍與顯示精度取決於檔案版本及內容。Web 版支援的其他格式尚未整合至本桌面專案。

## 快速開始

需要 **Windows x64**、支援 **Direct3D 11** 的顯示卡與驅動程式、**Git**、**Python 3.12+**，以及 Rust、**MSVC C++ 建置工具和 Windows SDK**。儲存庫透過 `rust-toolchain.toml` 固定 Rust **1.98.1**，相依版本以 `Cargo.lock` 為準。

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
python scripts/cargo.py run -p pomelo --locked
```

使用 **Ctrl+O**、拖放或啟動檔案路徑開啟電路板。在應用程式選單或工具列中選擇介面語言。

![歡迎畫面（英文介面）](images/welcome-en.png)

```powershell
python scripts/cargo.py run -p pomelo --locked -- --locale zh-TW --encoding windows-1252 "C:\boards\example.brd"
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

Cargo 包裝程式會在 `.cache/gpui/` 中準備本機 GPUI GPU 修補，不修改全域 Cargo registry。首次使用一般 Cargo 或 rust-analyzer 前，執行 `python scripts/prepare_gpui.py`。詳見[修補說明](patches/gpui/README.md)。一般桌面執行不需要 Node.js 或 Web 儲存庫。macOS 與 Linux 屬於後續規劃；目前原生應用程式與 GPU 後端以 Windows 為目標。

```powershell
python scripts/cargo.py build -p pomelo --locked
python scripts/cargo.py test --workspace --locked
python scripts/cargo.py clippy --workspace --all-targets --locked -- -D warnings
python scripts/cargo.py run -p pomelo-import --bin pcb_inspect --locked -- --help
```

| 目錄 | 用途 |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | 電路板模型、搜尋、選取與國際化 |
| [crates/pomelo-import](crates/pomelo-import) | Allegro 解析與匯入診斷 |
| [crates/pomelo-render](crates/pomelo-render) | 場景準備、D3D11 繪製與 HLSL 著色器 |
| [crates/pomelo](crates/pomelo) | GPUI 桌面應用程式與文件管理 |
| [locales](locales) | 五種語言的介面資源 |
| [docs](docs) | 研發計畫與驗證證據 |

- [研發計畫](docs/native-pcb-viewer-development-plan.md)
- [開發進度與驗證範圍](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU / ADR](docs/adr/0001-native-gpu-rendering.md)

## 參與貢獻

歡迎改善 Allegro 相容性、繪製精度、效能、翻譯與文件。[回報問題](https://github.com/HaiwenZhang/Pomelo-Desktop/issues)時，請附上來源工具與檔案版本、Windows 和 GPU 資訊、重現步驟及匯入診斷。小型範例或對照截圖有助於定位問題；分享前請移除機密設計資料。解析或幾何變更應提供針對性的回歸測試，視覺變更應附截圖，並保持五種語言的 README 同步。

## 授權

專案程式碼採用 [MIT 授權](LICENSE)。內附字型保留各自的授權，詳見[思源黑體](assets/fonts/source-han-sans/README.md)與[筆畫字型](assets/fonts/stroke/README.md)。
