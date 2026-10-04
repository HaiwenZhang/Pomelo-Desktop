#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/packaging-common.sh"

usage() {
    echo 'Usage: scripts/build-macos.sh [--target TARGET] [--toolchain NAME] [--offline] [--output-dir PATH] [--binary PATH]'
    echo 'Builds a signed Pomelo.app, app ZIP, and drag-to-install DMG. Requires ImageMagick.'
    echo 'MACOS_SIGNING_IDENTITY defaults to ad-hoc signing; notarization is not performed.'
}
packaging_init
packaging_arguments "$@"
[ "$(uname -s)" = Darwin ] || die 'Run this script on macOS.'
if [ -z "$TARGET" ]; then
    case "$(uname -m)" in
        arm64) TARGET=aarch64-apple-darwin ;;
        x86_64) TARGET=x86_64-apple-darwin ;;
        *) die 'Unsupported macOS architecture.' ;;
    esac
fi
case "$TARGET" in
    aarch64-apple-darwin) ARCH=arm64 ;;
    x86_64-apple-darwin) ARCH=x64 ;;
    *) die 'Target must be aarch64-apple-darwin or x86_64-apple-darwin.' ;;
esac
for tool in magick iconutil codesign hdiutil ditto lipo otool plutil; do require_command "$tool"; done
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-14.0}
[[ "$MACOSX_DEPLOYMENT_TARGET" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] || die 'Invalid MACOSX_DEPLOYMENT_TARGET.'
packaging_build
case "$ARCH:$(lipo -archs "$BINARY")" in
    arm64:arm64|x64:x86_64) ;;
    *) die 'Binary architecture does not match the requested target.' ;;
esac
# The app bundles no external dylibs; fail rather than ship a Homebrew-dependent app.
otool -L "$BINARY" | awk 'NR > 1 { print $1 }' | while IFS= read -r dependency; do
    case "$dependency" in
        /usr/lib/*|/System/Library/*) ;;
        *) die "Unbundled dynamic library: $dependency" ;;
    esac
done

STAGING=$(mktemp -d "${TMPDIR:-/tmp}/pomelo-macos.XXXXXX")
trap 'rm -rf "$STAGING"' EXIT
APP="$STAGING/image/Pomelo.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$STAGING/Pomelo.iconset"
cp "$BINARY" "$APP/Contents/MacOS/pomelo"
chmod 755 "$APP/Contents/MacOS/pomelo"
packaging_licenses "$APP/Contents/Resources/licenses"
for pixels in 16 32 128 256 512; do
    magick -background none "$ROOT/crates/pomelo/assets/pomelo.svg" -resize "${pixels}x${pixels}" "$STAGING/Pomelo.iconset/icon_${pixels}x${pixels}.png"
    retina=$((pixels * 2))
    magick -background none "$ROOT/crates/pomelo/assets/pomelo.svg" -resize "${retina}x${retina}" "$STAGING/Pomelo.iconset/icon_${pixels}x${pixels}@2x.png"
done
iconutil -c icns "$STAGING/Pomelo.iconset" -o "$APP/Contents/Resources/pomelo.icns"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Pomelo</string>
<key>CFBundleDisplayName</key><string>Pomelo</string>
<key>CFBundleIdentifier</key><string>io.github.haiwenzhang.Pomelo</string>
<key>CFBundleExecutable</key><string>pomelo</string>
<key>CFBundleIconFile</key><string>pomelo.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>${VERSION%%[-+]*}</string>
<key>CFBundleVersion</key><string>${VERSION%%[-+]*}</string>
<key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
EOF
plutil -lint "$APP/Contents/Info.plist"
IDENTITY=${MACOS_SIGNING_IDENTITY:--}
if [ "$IDENTITY" = - ]; then
    codesign --force --sign - --timestamp=none "$APP"
else
    codesign --force --sign "$IDENTITY" --options runtime --timestamp "$APP"
fi
codesign --verify --deep --strict "$APP"
ln -s /Applications "$STAGING/image/Applications"
NAME="Pomelo-$VERSION-macos-$ARCH"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUTPUT_DIR/$NAME.app.zip"
hdiutil create -ov -volname "Pomelo $VERSION" -srcfolder "$STAGING/image" -fs HFS+ -format UDZO "$OUTPUT_DIR/$NAME.dmg"
printf 'Packages:\n%s\n%s\n' "$OUTPUT_DIR/$NAME.app.zip" "$OUTPUT_DIR/$NAME.dmg"
