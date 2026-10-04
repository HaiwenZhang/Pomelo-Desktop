#!/usr/bin/env bash
# Shared helpers for native release packaging; compatible with macOS Bash 3.2.

die() { printf 'Error: %s\n' "$*" >&2; exit 1; }
require_command() { command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"; }

packaging_init() {
    ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
    cd "$ROOT"
    VERSION=$(awk '
        /^\[workspace.package\]$/ { package = 1; next }
        /^\[/ { package = 0 }
        package && /^version[[:space:]]*=/ {
            sub(/^[^=]*=[[:space:]]*"/, ""); sub(/".*$/, ""); print; exit
        }
    ' Cargo.toml)
    [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]] || die "Invalid workspace version: $VERSION"
    OUTPUT_DIR="$ROOT/dist"
    TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}
    TOOLCHAIN=
    OFFLINE=
    BINARY=
    TARGET=
}

packaging_arguments() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --help|-h) usage; exit 0 ;;
            --offline) OFFLINE=--offline; shift ;;
            --output-dir|--toolchain|--binary|--target)
                [ "$#" -ge 2 ] && [ -n "$2" ] || die "Missing value for $1"
                case "$1" in
                    --output-dir) OUTPUT_DIR=$2 ;;
                    --toolchain) TOOLCHAIN=$2 ;;
                    --binary) BINARY=$2 ;;
                    --target) TARGET=$2 ;;
                esac
                shift 2 ;;
            *) die "Unknown option: $1" ;;
        esac
    done
}

packaging_build() {
    mkdir -p "$OUTPUT_DIR"
    OUTPUT_DIR=$(cd "$OUTPUT_DIR" && pwd)
    if [ -z "$BINARY" ]; then
        require_command cargo
        mkdir -p "$TARGET_DIR"
        TARGET_DIR=$(cd "$TARGET_DIR" && pwd)
        local args=()
        [ -z "$TOOLCHAIN" ] || args+=("+$TOOLCHAIN")
        args+=(build -p pomelo --release --locked --target "$TARGET" --target-dir "$TARGET_DIR")
        [ -z "$OFFLINE" ] || args+=("$OFFLINE")
        cargo "${args[@]}"
        BINARY="$TARGET_DIR/$TARGET/release/pomelo"
    fi
    [ -f "$BINARY" ] && [ -x "$BINARY" ] || die "Executable not found: $BINARY"
    BINARY=$(cd "$(dirname "$BINARY")" && pwd)/$(basename "$BINARY")
}

packaging_licenses() {
    mkdir -p "$1"
    cp "$ROOT/LICENSE" "$1/LICENSE"
    cp "$ROOT/assets/fonts/source-han-sans/LICENSE.txt" "$1/Source-Han-Sans-OFL.txt"
    cp "$ROOT/crates/pomelo-core/src/search/data/LICENSE" "$1/Unicode-License.txt"
}
