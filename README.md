# Clipboard History Manager

A lightweight clipboard history manager for macOS, built in Rust. Runs as a menu bar app, records everything you copy, and lets you search and re-paste from history with a single hotkey.

## Features

- **Global hotkey** — Press `Cmd+Shift+V` to open the picker from anywhere
- **Fuzzy search** — Start typing to filter your clipboard history instantly
- **Keyboard navigation** — Arrow keys to browse, Enter to paste, Escape to close
- **Text & images** — Stores both text and image clipboard entries
- **Deduplication** — Duplicate entries are automatically moved to the top
- **System tray** — Menu bar icon with quick access to show, clear history, or quit
- **Fullscreen overlay** — Picker window floats over fullscreen apps (macOS)
- **Focus restore** — Automatically returns focus to the previous app after pasting
- **Auto-paste** — Simulates `Cmd+V` after selecting an item, so it pastes instantly into the target app

## Requirements

- macOS (uses native AppKit APIs for fullscreen overlay and focus management)
- Rust 1.85+ (edition 2021)
- **Accessibility permission** — Required for auto-paste (`Cmd+V` simulation). The app will prompt on first launch; grant access in **System Settings → Privacy & Security → Accessibility**.

## Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

The release binary is at `target/release/clipboard-history-manager`.

## Run

```bash
cargo run
```

Or run the release binary directly:

```bash
./target/release/clipboard-history-manager
```

The app will appear as a clipboard icon in your menu bar.

## Install as a macOS App

Create an app bundle so you can add it to Login Items:

```bash
# Build release binary
cargo build --release

# Create the app bundle
mkdir -p ~/Applications/ClipboardHistoryManager.app/Contents/MacOS

# Copy binary into bundle
cp target/release/clipboard-history-manager \
   ~/Applications/ClipboardHistoryManager.app/Contents/MacOS/ClipboardHistoryManager

# Create Info.plist
cat > ~/Applications/ClipboardHistoryManager.app/Contents/Info.plist << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>ClipboardHistoryManager</string>
    <key>CFBundleIdentifier</key>
    <string>com.ahmadgraham.clipboard-history-manager</string>
    <key>CFBundleName</key>
    <string>Clipboard History Manager</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
EOF

# Ad-hoc codesign so macOS can track Accessibility permissions across rebuilds
codesign -f -s - ~/Applications/ClipboardHistoryManager.app
```

### Run on Startup

1. Open **System Settings** → **General** → **Login Items & Extensions**
2. Click **+** under "Open at Login"
3. Press `Cmd+Shift+G`, type `~/Applications`, and select **ClipboardHistoryManager.app**

### Update After Rebuilding

```bash
cargo build --release && \
  cp target/release/clipboard-history-manager \
    ~/Applications/ClipboardHistoryManager.app/Contents/MacOS/ClipboardHistoryManager && \
  codesign -f -s - ~/Applications/ClipboardHistoryManager.app && \
  tccutil reset Accessibility com.ahmadgraham.clipboard-history-manager
```

After updating, re-grant **Accessibility** permission in System Settings → Privacy & Security → Accessibility (the rebuild invalidates the previous grant).

## Usage

| Action | Shortcut |
|---|---|
| Open picker | `Cmd+Shift+V` |
| Search | Just start typing |
| Navigate | `↑` / `↓` |
| Paste selected item | `Enter` |
| Close picker | `Escape` |

You can also click any item in the list to copy it, or use the system tray menu.

## Architecture

| File | Purpose |
|---|---|
| `main.rs` | Entry point — spawns clipboard poller, registers hotkey, runs event loop |
| `app.rs` | Core application state, window lifecycle, wgpu rendering pipeline |
| `ui.rs` | egui-based picker UI — search bar, item list, keyboard/mouse handling |
| `clipboard.rs` | Background clipboard polling, entry types (text/image), copy-to-clipboard |
| `history.rs` | Bounded deque with deduplication and fuzzy search |
| `hotkey.rs` | Global hotkey registration (Cmd+Shift+V) |
| `tray.rs` | System tray icon and menu |

## Dependencies

- **winit** — Cross-platform window creation and event loop
- **egui / egui-wgpu / egui-winit** — Immediate-mode GUI with GPU rendering
- **wgpu** — GPU abstraction (Metal on macOS)
- **arboard** — Cross-platform clipboard access with image support
- **global-hotkey** — System-wide hotkey registration
- **tray-icon** — System tray / menu bar icon
- **sublime_fuzzy** — Fuzzy string matching for search
- **objc2 / objc2-app-kit** — macOS native API bindings for NSWindow/NSPanel overlay

## License

MIT
