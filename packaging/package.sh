#!/bin/bash
# package.sh — Build Clipboard History Manager and package it as a .dmg
#
# Usage:
#   ./packaging/package.sh              # Build and create DMG
#   ./packaging/package.sh --universal  # Build universal binary (Intel + Apple Silicon)
#
# Output: dist/ClipboardHistoryManager-<version>.dmg
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_NAME="Clipboard History Manager"
BUNDLE_NAME="ClipboardHistoryManager"
BUNDLE_ID="com.ahmadgraham.clipboard-history-manager"

# Read version from Cargo.toml
VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "==> Building $APP_NAME v$VERSION"

UNIVERSAL=false
if [[ "${1:-}" == "--universal" ]]; then
    UNIVERSAL=true
fi

# ── 1. Build ──────────────────────────────────────────────────────────────────

cd "$PROJECT_DIR"

if $UNIVERSAL; then
    echo "==> Building universal binary (arm64 + x86_64)..."
    # Ensure both targets are installed
    rustup target add aarch64-apple-darwin x86_64-apple-darwin 2>/dev/null || true
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    BINARY=$(mktemp)
    lipo -create \
        target/aarch64-apple-darwin/release/clipboard-history-manager \
        target/x86_64-apple-darwin/release/clipboard-history-manager \
        -output "$BINARY"
    echo "    Universal binary created"
else
    echo "==> Building release binary..."
    cargo build --release
    BINARY="$PROJECT_DIR/target/release/clipboard-history-manager"
fi

# ── 2. Assemble .app bundle ──────────────────────────────────────────────────

DIST_DIR="$PROJECT_DIR/dist"
APP_DIR="$DIST_DIR/$BUNDLE_NAME.app"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Binary
cp "$BINARY" "$APP_DIR/Contents/MacOS/$BUNDLE_NAME"
chmod +x "$APP_DIR/Contents/MacOS/$BUNDLE_NAME"

# Info.plist (update version dynamically)
sed "s|<string>1.0.0</string>|<string>$VERSION</string>|" \
    "$SCRIPT_DIR/Info.plist" > "$APP_DIR/Contents/Info.plist"

# Icon
if [[ -f "$SCRIPT_DIR/AppIcon.icns" ]]; then
    cp "$SCRIPT_DIR/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"
else
    echo "    ⚠  No AppIcon.icns found — run generate_icon.sh first"
fi

# Clean up temp binary if universal
if $UNIVERSAL; then
    rm -f "$BINARY"
fi

# ── 3. Code-sign (ad-hoc) ────────────────────────────────────────────────────

echo "==> Code-signing (ad-hoc)..."
codesign --force --deep --sign - "$APP_DIR"

# ── 4. Create DMG ────────────────────────────────────────────────────────────

DMG_NAME="$BUNDLE_NAME-$VERSION.dmg"
DMG_PATH="$DIST_DIR/$DMG_NAME"

echo "==> Creating DMG..."
rm -f "$DMG_PATH"

# Create a temporary folder for DMG contents
DMG_STAGING="$DIST_DIR/_dmg_staging"
rm -rf "$DMG_STAGING"
mkdir -p "$DMG_STAGING"
cp -R "$APP_DIR" "$DMG_STAGING/"

# Create a symlink to /Applications for drag-and-drop install
ln -s /Applications "$DMG_STAGING/Applications"

# Build the DMG
hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$DMG_STAGING" \
    -ov \
    -format UDZO \
    "$DMG_PATH" \
    >/dev/null

rm -rf "$DMG_STAGING"

echo ""
echo "====================================="
echo "  ✅ $DMG_NAME created!"
echo "  📁 $DMG_PATH"
echo "  📦 Size: $(du -h "$DMG_PATH" | cut -f1)"
echo "====================================="
echo ""
echo "To distribute:"
echo "  1. Send the .dmg file to your friends"
echo "  2. They open it, drag the app to Applications"
echo "  3. Right-click → Open on first launch (to bypass Gatekeeper)"
echo "  4. Grant Accessibility permission when prompted"
