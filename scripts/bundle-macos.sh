#!/usr/bin/env bash
set -euo pipefail

# Builds netcheck-gui in release mode and wraps it in a standard macOS .app
# bundle so it can be double-clicked, added to the Dock/Applications, etc.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="NetCheck"
BUNDLE_ID="net.hugoh.netcheck"
VERSION="$(cd "$ROOT_DIR" && cargo metadata --no-deps --format-version=1 |
	python3 -c "import json,sys; print(next(p['version'] for p in json.load(sys.stdin)['packages'] if p['name']=='netcheck-gui'))")"

BUILD_DIR="$ROOT_DIR/target/release"
APP_DIR="$ROOT_DIR/target/${APP_NAME}.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

echo "Building netcheck-gui (release)..."
cargo build --release -p netcheck-gui --manifest-path "$ROOT_DIR/Cargo.toml"

echo "Assembling $APP_DIR..."
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cp "$BUILD_DIR/netcheck-gui" "$MACOS_DIR/$APP_NAME"

cat >"$CONTENTS_DIR/Info.plist" <<INFOPLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
</dict>
</plist>
INFOPLIST

if [ -f "$ROOT_DIR/assets/AppIcon.icns" ]; then
	cp "$ROOT_DIR/assets/AppIcon.icns" "$RESOURCES_DIR/AppIcon.icns"
else
	echo "No assets/AppIcon.icns found — bundling without a custom icon."
fi

# Ad-hoc sign (identity "-"): no Apple Developer account needed. This
# satisfies codesign -v / hardened-runtime checks and lets the app run
# locally without a Gatekeeper prompt (local builds aren't quarantined
# anyway). It is NOT trusted by Gatekeeper on any other machine — sharing
# the app with someone else still needs a real Developer ID + notarization.
echo "Ad-hoc signing..."
codesign --force --deep --sign - "$APP_DIR"

echo "Done: $APP_DIR"
echo "Run with: open \"$APP_DIR\""
