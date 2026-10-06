<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>すべての接続を、レイヤーごとに見渡す。</strong><br>
  GPUI と GPUI Kit を使った、ローカルで動作する読み取り専用 PCB ビューアーです。
</p>

<p align="center"><a href="README.md">English</a> · <a href="README_zh-CN.md">简体中文</a> · <a href="README_zh-TW.md">繁體中文</a> · 日本語 · <a href="README_ko.md">한국어</a></p>

<p align="center"><a href="https://github.com/HaiwenZhang/Pomelo-Desktop/releases">リリースをダウンロード</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D4" alt="Windows, macOS, Linux">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11%20%7C%20Metal%20%7C%20wgpu-58752c" alt="D3D11, Metal, wgpu">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

## 基板をより見やすく

- **設計データはローカルに。** 基板ファイルをアップロードせずに読み込み、表示できます。元のファイルは変更しません。
- **ネイティブ GPU 描画。** D3D11/HLSL、Metal/MSL、wgpu/WGSL で配線、円弧、パッド、ビア、銅箔と穴、図形、MSDF による基板テキストを描画します。
- **接続を追跡。** ネットや部品を検索し、結果に移動して選択したオブジェクトの属性を確認できます。
- **表示を調整。** レイヤーの管理、レイヤー別・ネット別の色分け、銅箔の不透明度、パッドの塗りつぶし、ラベルを設定できます。
- **複数の基板を扱う。** 複数ドキュメント、最近使ったファイル、表示設定の保存に対応しています。
- **言語とテーマを選ぶ。** 英語、簡体字中国語、繁体字中国語、日本語、韓国語の UI と、ライト・ダークテーマを利用できます。

> **開発中：** 互換性、描画精度、性能、各プラットフォームの安定性には追加検証が必要です。読み取り専用で、基板の編集や DRC は行いません。[開発情報](docs/development-progress.md)を参照してください。

## 対応形式

`.brd`、`.PcbDoc`、ODB++ アーカイブ（`.tgz`、`.tar`、`.tar.gz`）、`.pcb`、`.kicad_pcb`、`edb.def` に対応します。読み込み範囲はファイルのバージョンと保存済みデータに依存し、未対応の内容は診断として表示します。

すべての形式は `crates/pomelo-import` の Rust 実装で読み込み、JavaScript エンジンや Node.js は使用しません。

## クイックスタート

Git と、リポジトリで固定された Rust **1.98.1** が必要です。依存関係のバージョンは `Cargo.lock` に記録されています。

| プラットフォーム | ビルドと実行の要件 |
| --- | --- |
| Windows x64 | MSVC C++ ビルドツール、Windows SDK、Direct3D 11 対応 GPU とドライバー |
| macOS | Metal 対応 Mac、Xcode Command Line Tools |
| Linux | fontconfig、FreeType、xkbcommon、Wayland/X11、ALSA、OpenSSL の開発パッケージと Vulkan 対応ドライバー |

以下のコマンドは各プラットフォームで使用できます。macOS のアプリ操作では Ctrl の代わりに Cmd を使用します。

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
cargo run -p pomelo --locked
```

**Ctrl+O**、ドラッグ＆ドロップ、または起動時のファイルパス指定で基板を開きます。アプリのメニューまたはツールバーから表示言語を選択できます。

```powershell
cargo run -p pomelo --locked -- --locale ja --encoding windows-1252 "example.brd"
```

`--locale` は `en`、`zh-CN`、`zh-TW`、`ja`、`ko` に対応し、今回の起動だけに適用されます。保存済みの言語設定は変更しません。`--encoding` は `auto`、`utf-8`、`gbk`、`shift_jis`、`big5`、`windows-1252` に対応し、既定値は `auto` です。自動判定が不正確な場合は明示的に指定してください。表示言語とファイルの文字コードは独立しています。文字コード指定は同じプロセスで開くファイルに適用され、`--` 以降の引数はファイルパスとして扱われます。

## ショートカット

| 操作 | キー |
| --- | --- |
| ファイルを開く | `Ctrl+O` |
| ドキュメントを閉じる | `Ctrl+W` |
| ドキュメントを再読み込み | `Ctrl+R` |
| 次／前のドキュメント | `Ctrl+Tab / Ctrl+Shift+Tab` |
| 基板全体を表示 | `F2` |
| 検索にフォーカス | `Ctrl+K` |
| 設定 | `Ctrl+,` |
| 左／右パネルの切り替え | `Ctrl+B / Ctrl+Shift+B` |

## ビルドと開発

依存関係のバージョンと取得元は `Cargo.toml` と `Cargo.lock` に記録されています。[GPUI 依存関係](docs/gpui-dependencies.md)と [GPU バックエンド](docs/native-gpu-backends.md)を参照してください。

```powershell
cargo build -p pomelo --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p pomelo-import --bin pcb_inspect --locked -- --help
```

### Release パッケージ

| プラットフォーム／構成 | 形式 | コマンド |
| --- | --- | --- |
| Windows x64 | `.exe` | `scripts\build-windows.bat` |
| Windows ARM64 | `.exe` | `scripts\build-windows.bat --arch arm64` |
| macOS arm64 / x64 | `.dmg`, `.app.zip` | `scripts/build-macos.sh` |
| Ubuntu 24.04+ amd64 / arm64 | `.deb`, `.tar.gz` | `scripts/build-ubuntu.sh` |

Rust と各 OS のビルド依存関係が必要です。macOS は ImageMagick、Windows は Inno Setup 6、Ubuntu は `dpkg-dev` と `desktop-file-utils` も必要です。出力先は `dist` です。

Shell スクリプトは `--offline`、`--toolchain NAME`、`--output-dir PATH`、既存バイナリ用の `--binary PATH` に対応します。正式ビルドは `cargo build --release --locked` を使用し、Python は不要です。

Ubuntu Release は 24.04 でビルドして ABI 基準を維持し、CI で 26.04 へのインストールと実行も確認します。新しい Ubuntu でのローカルビルドは、新しいライブラリを要求する場合があります。

macOS は既定でアドホック署名、最低 macOS 14.0 です。`MACOS_SIGNING_IDENTITY` でキーチェーンの署名 ID を指定できます。Apple の公証は未設定です。

[Release ワークフロー](.github/workflows/release.yml)：`vX.Y.Z` タグを push すると全 OS をビルドし、検証後にパッケージと `SHA256SUMS` を公開します。タグは `Cargo.toml` の workspace バージョンと一致させてください。手動実行は Actions アーティファクトのみ生成します。

## プロジェクト構成とドキュメント

| 場所 | 内容 |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | 基板モデル、検索、選択、国際化 |
| [crates/pomelo-import](crates/pomelo-import) | ファイル読み込みと診断 |
| [crates/pomelo-render](crates/pomelo-render) | シーン準備、D3D11・Metal・wgpu 描画 |
| [crates/pomelo](crates/pomelo) | GPUI デスクトップアプリとドキュメント管理 |
| [locales](locales) | 5 言語の UI リソース |
| [docs](docs) | 一般的な開発情報 |

- [ドキュメント](docs/README.md)
- [開発状況と検証範囲](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU](docs/native-gpu-backends.md)

## 開発計画

以下は開発予定または追加実装が必要な機能であり、提供済みの機能ではありません。

| 分野 | 計画内容 |
| --- | --- |
| 日常のレビュー | 2 点・折れ線測定、オブジェクト間距離、複数選択、保存可能なハイライトグループ |
| 表示と出力 | 名前付きビュー、移動履歴、表示設定、スクリーンショット、軽量なローカル出力 |
| 設計情報 | 属性検索、層別パッド構造、物理積層、ネットグループ、接続構造、指定経路長 |
| 継続的な改善 | ファイル互換性、大規模シーンの性能、各 OS の安定性、キーボード操作、5 言語の UI |

優先順位と範囲は[機能計画](docs/viewer-review-capabilities-plan.md)、実装と検証条件は[開発設計](docs/viewer-review-development-design.md)を参照してください。

読み取り専用の表示とレビューを対象とし、元の設計の編集や DRC は行いません。

## 貢献

ファイル互換性、描画精度、性能、翻訳、ドキュメントへの貢献を歓迎します。[不具合の報告](https://github.com/HaiwenZhang/Pomelo-Desktop/issues)には、元のツールとファイルのバージョン、OS と GPU の情報、再現手順、インポート診断を添えてください。小さなサンプルや比較画像が役立ちます。共有前に機密の設計データを除去してください。解析や幾何処理の変更には対象を絞った回帰テストを、表示変更にはスクリーンショットを添え、5 言語の README を同期してください。

## ライセンス

プロジェクトのコードは [MIT ライセンス](LICENSE)です。同梱フォントにはそれぞれのライセンスが適用されます。[Source Han Sans](assets/fonts/source-han-sans/README.md)を参照してください。

## 謝辞

Pomelo Desktop の GPU アクセラレーション対応 UI フレームワークと UI コンポーネントを提供する [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) および [GPUI Kit](https://github.com/longbridge/gpui-kit) プロジェクトに感謝します。
