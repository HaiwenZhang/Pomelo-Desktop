#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/packaging-common.sh"

usage() {
    echo 'Usage: scripts/build-ubuntu.sh [--toolchain NAME] [--offline] [--output-dir PATH] [--binary PATH]'
    echo 'Builds a native .deb and .tar.gz on Ubuntu 24.04 or later.'
    echo 'Use Ubuntu 24.04 for release builds that must also run on Ubuntu 24.04.'
}
packaging_init
packaging_arguments "$@"
[ "$(uname -s)" = Linux ] || die 'Run this script on Ubuntu.'
DISTRO=$(source /etc/os-release; printf '%s' "$ID")
UBUNTU_VERSION=$(source /etc/os-release; printf '%s' "$VERSION_ID")
[ "$DISTRO" = ubuntu ] || die 'This package targets Ubuntu 24.04 and later.'
dpkg --compare-versions "$UBUNTU_VERSION" ge 24.04 || die 'Ubuntu 24.04 or later is required.'
for tool in dpkg-deb dpkg-shlibdeps readelf desktop-file-validate gzip tar; do require_command "$tool"; done
ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) EXPECTED_TARGET=x86_64-unknown-linux-gnu ;;
    arm64) EXPECTED_TARGET=aarch64-unknown-linux-gnu ;;
    *) die 'Supported Ubuntu architectures: amd64, arm64.' ;;
esac
[ -z "$TARGET" ] || [ "$TARGET" = "$EXPECTED_TARGET" ] || die 'Cross-architecture Ubuntu packaging is not supported.'
TARGET=$EXPECTED_TARGET
packaging_build
export LC_ALL=C
case "$ARCH:$(readelf -h "$BINARY" | awk -F: '/Machine:/ { gsub(/^[[:space:]]+/, "", $2); print $2 }')" in
    'amd64:Advanced Micro Devices X86-64'|'arm64:AArch64') ;;
    *) die 'Binary architecture does not match the packaging host.' ;;
esac

STAGING=$(mktemp -d "${TMPDIR:-/tmp}/pomelo-ubuntu.XXXXXX")
trap 'rm -rf "$STAGING"' EXIT
PACKAGE="$STAGING/package"
mkdir -p "$PACKAGE/DEBIAN" "$PACKAGE/usr/bin" "$PACKAGE/usr/share/applications" "$PACKAGE/usr/share/icons/hicolor/scalable/apps" "$PACKAGE/usr/share/doc/pomelo" "$STAGING/debian"
install -m 755 "$BINARY" "$PACKAGE/usr/bin/pomelo"
install -m 644 "$ROOT/crates/pomelo/assets/pomelo.svg" "$PACKAGE/usr/share/icons/hicolor/scalable/apps/pomelo.svg"
packaging_licenses "$PACKAGE/usr/share/doc/pomelo"
cat > "$PACKAGE/usr/share/applications/io.github.haiwenzhang.Pomelo.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Pomelo
Comment=Native Cadence Allegro PCB viewer
Exec=pomelo %f
Icon=pomelo
Terminal=false
Categories=Graphics;Engineering;
Keywords=PCB;Allegro;Electronics;
StartupNotify=true
EOF
desktop-file-validate "$PACKAGE/usr/share/applications/io.github.haiwenzhang.Pomelo.desktop"
# Derive actual ABI dependencies from the binary and Ubuntu's installed packages.
cat > "$STAGING/debian/control" <<'EOF'
Source: pomelo
Section: electronics
Priority: optional
Maintainer: Haiwen Zhang <HaiwenZhang@users.noreply.github.com>

Package: pomelo
Architecture: any
Description: Native Cadence Allegro PCB viewer
EOF
DEPENDENCIES=$(cd "$STAGING" && dpkg-shlibdeps -O -e"$PACKAGE/usr/bin/pomelo")
case "$DEPENDENCIES" in shlibs:Depends=*) ;; *) die 'Unexpected dpkg-shlibdeps output.' ;; esac
DEPENDENCIES=${DEPENDENCIES#shlibs:Depends=}
[ -n "$DEPENDENCIES" ] || die 'Could not determine ELF library dependencies.'
# GPUI/wgpu also load window-system and Vulkan libraries dynamically.
for package in libvulkan1 libfontconfig1 libfreetype6 libasound2t64 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-cursor0 libxcb1 libx11-6 libx11-xcb1; do
    case ", $DEPENDENCIES," in
        *", $package,"*|*", $package "*) ;;
        *) DEPENDENCIES="$DEPENDENCIES, $package" ;;
    esac
done
DEB_VERSION=$(printf '%s' "$VERSION" | sed 's/-/~/')
INSTALLED_SIZE=$(du -sk "$PACKAGE/usr" | awk '{print $1}')
cat > "$PACKAGE/DEBIAN/control" <<EOF
Package: pomelo
Version: $DEB_VERSION
Section: electronics
Priority: optional
Architecture: $ARCH
Maintainer: Haiwen Zhang <HaiwenZhang@users.noreply.github.com>
Installed-Size: $INSTALLED_SIZE
Depends: $DEPENDENCIES
Recommends: mesa-vulkan-drivers, xdg-desktop-portal
Homepage: https://github.com/HaiwenZhang/Pomelo-Desktop
Description: Native Cadence Allegro PCB viewer
 Explore Cadence Allegro binary boards using native GPU rendering.
EOF
NAME="Pomelo-$VERSION-ubuntu-$UBUNTU_VERSION-$ARCH"
dpkg-deb --build --root-owner-group "$PACKAGE" "$OUTPUT_DIR/$NAME.deb"
PORTABLE="$STAGING/$NAME"
mkdir -p "$PORTABLE"
cp -R "$PACKAGE/usr/." "$PORTABLE/"
cat > "$PORTABLE/README.txt" <<EOF
Pomelo $VERSION ($ARCH), built on Ubuntu $UBUNTU_VERSION.
Run bin/pomelo. The binary requires the following system packages:
$DEPENDENCIES
Install the .deb with apt for automatic dependency resolution and desktop integration.
EOF
tar -czf "$OUTPUT_DIR/$NAME.tar.gz" -C "$STAGING" "$NAME"
printf 'Packages:\n%s\n%s\n' "$OUTPUT_DIR/$NAME.deb" "$OUTPUT_DIR/$NAME.tar.gz"
