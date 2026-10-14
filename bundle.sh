#!/bin/sh
# Build the GUI and assemble dist/Place Cards.app (no cargo-bundle needed).
set -e
cd "$(dirname "$0")"
cargo build --release --bin place-cards-gui
APP="dist/Place Cards.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/place-cards-gui "$APP/Contents/MacOS/place-cards-gui"
# App icon (regenerate with assets/icon/make_icon.sh)
cp assets/icon/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Place Cards</string>
  <key>CFBundleDisplayName</key><string>Place Cards</string>
  <key>CFBundleIdentifier</key><string>local.place-cards.gui</string>
  <key>CFBundleExecutable</key><string>place-cards-gui</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Ad-hoc sign so Apple Silicon will launch it.
codesign --force --deep -s - "$APP" 2>/dev/null || true
# let Finder refresh the icon
touch "$APP"
echo "Built $APP"
