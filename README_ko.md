<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>모든 연결을 레이어별로 살펴보세요.</strong><br>
  Rust와 GPUI Kit로 만든 네이티브 PCB 뷰어입니다. Windows에서 파일을 로컬로 처리하고 GPU로 렌더링하여 Cadence Allegro 보드를 살펴볼 수 있습니다.
</p>

<p align="center"><a href="README.md">English</a> · <a href="README_zh-CN.md">简体中文</a> · <a href="README_zh-TW.md">繁體中文</a> · <a href="README_ja.md">日本語</a> · 한국어</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows-0078D4" alt="Windows">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11-58752c" alt="D3D11">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

![PCB 작업 공간 (영어 UI)](images/pcb-example-en.png)

## 보드를 더 선명하게 살펴보기

- **설계 파일은 로컬에 보관됩니다.** 업로드 없이 컴퓨터에서 보드를 가져오고 확인합니다. 원본 파일은 수정하지 않습니다.
- **네이티브 GPU 렌더링.** D3D11과 HLSL로 배선, 원호, 패드, 비아, 구리 영역과 구멍, 도형, MSDF 보드 텍스트를 표시합니다.
- **연결 추적.** 네트와 부품을 검색하고 결과로 이동하여 선택한 객체의 속성을 확인합니다.
- **표시 조정.** 레이어를 관리하고 레이어별 또는 네트별로 색상을 지정하며, 구리 불투명도, 패드 채우기, 라벨 표시를 조정합니다.
- **여러 보드 작업.** 다중 문서, 최근 파일, 보기 설정 저장을 지원합니다.
- **언어와 테마 선택.** 영어, 중국어 간체, 중국어 번체, 일본어, 한국어 UI와 밝은 테마 및 어두운 테마를 제공합니다.

> **개발 중:** 전체 BRD MVP 인수 검증은 아직 완료되지 않았습니다. 표시 정확도, 대형 보드 성능, DPI 및 입력기, 장시간 실행 안정성에는 추가 검증이 필요합니다. 읽기 전용 뷰어이며 보드를 편집하거나 DRC를 실행하지 않습니다. 검증 범위는 [개발 현황](docs/development-progress.md)을 참고하세요.

## 지원 형식

현재 **Cadence Allegro 바이너리 `.brd` 파일**을 지원합니다. 같은 확장자를 사용하는 다른 EDA 도구의 형식까지 지원한다는 뜻은 아닙니다. 가져오기 범위와 표시 정확도는 파일 버전과 내용에 따라 달라집니다. 웹 버전이 지원하는 다른 형식은 아직 이 데스크톱 프로젝트에 구현되지 않았습니다.

## 빠른 시작

**Windows x64**, **Direct3D 11**을 지원하는 GPU 및 드라이버, **Git**, **Python 3.12+**, Rust, **MSVC C++ 빌드 도구 및 Windows SDK**가 필요합니다. `rust-toolchain.toml`에서 Rust **1.98.1**을 고정하며, 의존성 버전은 `Cargo.lock`으로 관리합니다.

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
python scripts/cargo.py run -p pomelo --locked
```

**Ctrl+O**, 끌어서 놓기 또는 시작 시 파일 경로 지정으로 보드를 엽니다. 앱 메뉴나 도구 모음에서 UI 언어를 선택할 수 있습니다.

![시작 화면 (영어 UI)](images/welcome-en.png)

```powershell
python scripts/cargo.py run -p pomelo --locked -- --locale ko --encoding windows-1252 "C:\boards\example.brd"
```

`--locale`은 `en`, `zh-CN`, `zh-TW`, `ja`, `ko`를 지원합니다. 해당 실행에만 적용되며 저장된 언어 설정은 변경하지 않습니다. `--encoding`은 `utf-8`, `gbk`, `shift_jis`, `big5`, `windows-1252`를 지원하며 기본값은 엄격한 UTF-8입니다. 선택한 인코딩은 해당 프로세스에서 여는 파일에 적용됩니다. UI 언어와 원본 파일 인코딩은 서로 독립적입니다. `--` 뒤의 인수는 모두 파일 경로로 처리합니다.

## 단축키

| 동작 | 키 |
| --- | --- |
| 파일 열기 | `Ctrl+O` |
| 문서 닫기 | `Ctrl+W` |
| 문서 다시 불러오기 | `Ctrl+R` |
| 다음 / 이전 문서 | `Ctrl+Tab / Ctrl+Shift+Tab` |
| 보드 전체 보기 | `F2` |
| 검색에 초점 이동 | `Ctrl+K` |
| 설정 | `Ctrl+,` |
| 왼쪽 / 오른쪽 패널 전환 | `Ctrl+B / Ctrl+Shift+B` |

## 빌드 및 개발

Cargo 래퍼는 `.cache/gpui/`에 로컬 GPUI GPU 패치를 준비하며 전역 Cargo registry는 변경하지 않습니다. 일반 Cargo나 rust-analyzer를 처음 사용하기 전에 `python scripts/prepare_gpui.py`를 실행하세요. [패치 설명](patches/gpui/README.md)을 참고하세요. 일반적인 데스크톱 실행에는 Node.js나 웹 저장소가 필요하지 않습니다. macOS와 Linux는 향후 계획이며, 현재 네이티브 앱과 GPU 백엔드는 Windows를 대상으로 합니다.

```powershell
python scripts/cargo.py build -p pomelo --locked
python scripts/cargo.py test --workspace --locked
python scripts/cargo.py clippy --workspace --all-targets --locked -- -D warnings
python scripts/cargo.py run -p pomelo-import --bin pcb_inspect --locked -- --help
```

| 경로 | 용도 |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | 보드 모델, 검색, 선택 및 국제화 |
| [crates/pomelo-import](crates/pomelo-import) | Allegro 파싱 및 가져오기 진단 |
| [crates/pomelo-render](crates/pomelo-render) | 장면 준비, D3D11 렌더러 및 HLSL 셰이더 |
| [crates/pomelo](crates/pomelo) | GPUI 데스크톱 앱 및 문서 관리 |
| [locales](locales) | 5개 언어의 UI 리소스 |
| [docs](docs) | 개발 계획 및 검증 기록 |

- [개발 계획](docs/native-pcb-viewer-development-plan.md)
- [개발 현황 및 검증 범위](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU / ADR](docs/adr/0001-native-gpu-rendering.md)

## 기여

Allegro 호환성, 렌더링 정확도, 성능, 번역 및 문서 개선을 환영합니다. [문제 보고](https://github.com/HaiwenZhang/Pomelo-Desktop/issues)에는 원본 도구와 파일 버전, Windows 및 GPU 정보, 재현 단계, 가져오기 진단을 포함해 주세요. 작은 샘플이나 비교 스크린샷이 도움이 됩니다. 공유 전에 기밀 설계 데이터를 제거하세요. 파서나 기하 처리 변경에는 관련 회귀 테스트를, 시각적 변경에는 스크린샷을 추가하고 5개 언어의 README를 동기화해 주세요.

## 라이선스

프로젝트 코드는 [MIT 라이선스](LICENSE)를 따릅니다. 포함된 글꼴에는 각각의 라이선스가 적용됩니다. [Source Han Sans](assets/fonts/source-han-sans/README.md)와 [획 글꼴](assets/fonts/stroke/README.md)을 참고하세요.
