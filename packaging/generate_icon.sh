#!/bin/bash
# generate_icon.sh — Creates AppIcon.icns by rendering an emoji via Swift.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ICONSET_DIR="$SCRIPT_DIR/AppIcon.iconset"
ICNS_OUT="$SCRIPT_DIR/AppIcon.icns"
BASE_PNG="$SCRIPT_DIR/_base_1024.png"

rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

# Render emoji icon using Swift (ships with Xcode CLI tools)
swift - "$BASE_PNG" <<'SWIFT'
import AppKit

let outPath = CommandLine.arguments[1]
let size = 1024

guard let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: size, pixelsHigh: size,
    bitsPerSample: 8, samplesPerPixel: 4,
    hasAlpha: true, isPlanar: false,
    colorSpaceName: .calibratedRGB,
    bytesPerRow: 0, bitsPerPixel: 0
) else {
    fputs("Failed to create bitmap\n", stderr)
    exit(1)
}

NSGraphicsContext.saveGraphicsState()
let ctx = NSGraphicsContext(bitmapImageRep: rep)!
NSGraphicsContext.current = ctx

// Background: rounded rect with a blue-grey gradient feel
let bgRect = NSRect(x: 0, y: 0, width: size, height: size)
NSColor(red: 0.12, green: 0.13, blue: 0.16, alpha: 1.0).setFill()
NSBezierPath.fill(bgRect)

let inset: CGFloat = 20
let cornerRadius: CGFloat = CGFloat(size) * 0.22
let roundedPath = NSBezierPath(
    roundedRect: bgRect.insetBy(dx: inset, dy: inset),
    xRadius: cornerRadius, yRadius: cornerRadius
)
NSColor(red: 0.18, green: 0.30, blue: 0.55, alpha: 1.0).setFill()
roundedPath.fill()

// Draw clipboard emoji centered
let emoji = "📋" as NSString
let fontSize: CGFloat = 680
let font = NSFont.systemFont(ofSize: fontSize)
let attrs: [NSAttributedString.Key: Any] = [.font: font]
let strSize = emoji.size(withAttributes: attrs)
let x = (CGFloat(size) - strSize.width) / 2
let y = (CGFloat(size) - strSize.height) / 2
emoji.draw(at: NSPoint(x: x, y: y), withAttributes: attrs)

NSGraphicsContext.restoreGraphicsState()

guard let pngData = rep.representation(using: .png, properties: [:]) else {
    fputs("Failed to generate PNG\n", stderr)
    exit(1)
}
let url = URL(fileURLWithPath: outPath)
try! pngData.write(to: url)
print("Base icon rendered")
SWIFT

# Generate all iconset sizes from the 1024 base
for s in 16 32 64 128 256 512; do
    sips -z "$s" "$s" "$BASE_PNG" --out "$ICONSET_DIR/icon_${s}x${s}.png" >/dev/null 2>&1
done

# 1024x1024
cp "$BASE_PNG" "$ICONSET_DIR/icon_512x512@2x.png"

# @2x variants
for s in 16 32 128 256; do
    doubled=$((s * 2))
    sips -z "$doubled" "$doubled" "$BASE_PNG" --out "$ICONSET_DIR/icon_${s}x${s}@2x.png" >/dev/null 2>&1
done

rm -f "$BASE_PNG"

# Convert to .icns
iconutil -c icns "$ICONSET_DIR" -o "$ICNS_OUT"
rm -rf "$ICONSET_DIR"

echo "Created $ICNS_OUT"
