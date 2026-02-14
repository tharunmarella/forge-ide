#!/bin/bash
set -euo pipefail

# Package Forge IDE as a macOS .app bundle for Apple Silicon (arm64)
# Usage: ./scripts/package-macos.sh [--dmg] [--skip-build]

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="Forge IDE"
BUNDLE_NAME="Forge-IDE.app"
VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/' || echo "0.4.6")
# Read version from workspace
VERSION=$(cargo metadata --manifest-path "$PROJECT_DIR/Cargo.toml" --no-deps --format-version 1 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['packages'][0]['version'])" 2>/dev/null || echo "0.4.6")

TARGET="aarch64-apple-darwin"
BUILD_DIR="$PROJECT_DIR/target/release"
DIST_DIR="$PROJECT_DIR/dist"
APP_DIR="$DIST_DIR/$BUNDLE_NAME"

CREATE_DMG=false
SKIP_BUILD=false

for arg in "$@"; do
    case $arg in
        --dmg) CREATE_DMG=true ;;
        --skip-build) SKIP_BUILD=true ;;
        --help)
            echo "Usage: $0 [--dmg] [--skip-build]"
            echo "  --dmg         Also create a .dmg disk image"
            echo "  --skip-build  Skip cargo build (use existing binaries)"
            exit 0
            ;;
    esac
done

echo "==> Packaging $APP_NAME v$VERSION for macOS arm64"

# Step 1: Build release binaries
if [ "$SKIP_BUILD" = false ]; then
    echo "==> Building release binaries (this may take a while)..."
    cd "$PROJECT_DIR"
    cargo build --release --target "$TARGET"
    BUILD_DIR="$PROJECT_DIR/target/$TARGET/release"
else
    # Check if cross-compiled binaries exist, otherwise fall back to default release
    if [ -f "$PROJECT_DIR/target/$TARGET/release/lapce" ]; then
        BUILD_DIR="$PROJECT_DIR/target/$TARGET/release"
    else
        BUILD_DIR="$PROJECT_DIR/target/release"
    fi
    echo "==> Skipping build, using binaries from $BUILD_DIR"
fi

# Verify binaries exist
if [ ! -f "$BUILD_DIR/lapce" ]; then
    echo "ERROR: Binary not found at $BUILD_DIR/lapce"
    echo "Run without --skip-build to compile first."
    exit 1
fi

# Verify architecture
ARCH=$(lipo -archs "$BUILD_DIR/lapce" 2>/dev/null || echo "unknown")
echo "==> Binary architecture: $ARCH"

# Step 2: Create .app bundle structure
echo "==> Creating app bundle..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Step 3: Copy binaries
cp "$BUILD_DIR/lapce" "$APP_DIR/Contents/MacOS/lapce"
cp "$BUILD_DIR/lapce-proxy" "$APP_DIR/Contents/MacOS/lapce-proxy"

# Strip debug symbols to reduce size
strip "$APP_DIR/Contents/MacOS/lapce" 2>/dev/null || true
strip "$APP_DIR/Contents/MacOS/lapce-proxy" 2>/dev/null || true

# Step 4: Copy Info.plist (update names for Forge IDE)
cat > "$APP_DIR/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>lapce</string>
  <key>CFBundleIdentifier</key>
  <string>dev.forge-ide</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Forge IDE</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
PLIST

# Inject version dynamically
echo "  <string>$VERSION</string>" >> "$APP_DIR/Contents/Info.plist"

cat >> "$APP_DIR/Contents/Info.plist" << 'PLIST'
  <key>CFBundleSupportedPlatforms</key>
  <array>
    <string>MacOSX</string>
  </array>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>CFBundleIconFile</key>
  <string>lapce.icns</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
  <key>CFBundleDisplayName</key>
  <string>Forge IDE</string>
  <key>NSRequiresAquaSystemAppearance</key>
  <string>NO</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>LSArchitecturePriority</key>
  <array>
    <string>arm64</string>
  </array>
  <key>NSAppleEventsUsageDescription</key>
  <string>An application in Forge IDE would like to access AppleScript.</string>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key>
      <string>Forge IDE Auth</string>
      <key>CFBundleURLSchemes</key>
      <array>
        <string>forge-ide</string>
      </array>
    </dict>
  </array>
</dict>
</plist>
PLIST

# Step 5: Copy icon
cp "$PROJECT_DIR/extra/macos/Lapce.app/Contents/Resources/lapce.icns" "$APP_DIR/Contents/Resources/lapce.icns"

# Step 6: Create PkgInfo
echo -n "APPL????" > "$APP_DIR/Contents/PkgInfo"

# Step 7: Ad-hoc codesign (required for arm64 macOS)
echo "==> Code signing (ad-hoc)..."
codesign --force --deep --sign - "$APP_DIR"

echo "==> App bundle created at: $APP_DIR"

# Report sizes
LAPCE_SIZE=$(du -sh "$APP_DIR/Contents/MacOS/lapce" | cut -f1)
PROXY_SIZE=$(du -sh "$APP_DIR/Contents/MacOS/lapce-proxy" | cut -f1)
TOTAL_SIZE=$(du -sh "$APP_DIR" | cut -f1)
echo "    lapce binary:       $LAPCE_SIZE"
echo "    lapce-proxy binary: $PROXY_SIZE"
echo "    Total bundle size:  $TOTAL_SIZE"

# Step 8: Optionally create DMG
if [ "$CREATE_DMG" = true ]; then
    echo "==> Creating DMG..."
    DMG_NAME="Forge-IDE-${VERSION}-arm64.dmg"
    DMG_PATH="$DIST_DIR/$DMG_NAME"
    STAGING_DIR="$DIST_DIR/dmg-staging"

    rm -rf "$STAGING_DIR" "$DMG_PATH"
    mkdir -p "$STAGING_DIR"

    cp -R "$APP_DIR" "$STAGING_DIR/"
    ln -s /Applications "$STAGING_DIR/Applications"

    hdiutil create -volname "Forge IDE" \
        -srcfolder "$STAGING_DIR" \
        -ov -format UDZO \
        "$DMG_PATH"

    rm -rf "$STAGING_DIR"

    DMG_SIZE=$(du -sh "$DMG_PATH" | cut -f1)
    echo "==> DMG created at: $DMG_PATH ($DMG_SIZE)"
fi

echo ""
echo "==> Done! To install:"
echo "    cp -R \"$APP_DIR\" /Applications/"
echo ""
echo "    Or drag Forge-IDE.app to your Applications folder."