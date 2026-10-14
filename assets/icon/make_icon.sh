#!/bin/sh
# Regenerates assets/icon/AppIcon.icns, AppIcon-1024.png and icon-256.png from make_icon.swift.
# The .iconset folder is built under target/ (not kept in the repo).
set -e
cd "$(dirname "$0")"
SET=../../target/AppIcon.iconset
rm -rf "$SET"; mkdir -p "$SET"
swiftc -O make_icon.swift -o ../../target/make_icon
../../target/make_icon \
  1024=AppIcon-1024.png 256=icon-256.png \
  16="$SET/icon_16x16.png" 32="$SET/icon_16x16@2x.png" 32="$SET/icon_32x32.png" 64="$SET/icon_32x32@2x.png" \
  128="$SET/icon_128x128.png" 256="$SET/icon_128x128@2x.png" 256="$SET/icon_256x256.png" \
  512="$SET/icon_256x256@2x.png" 512="$SET/icon_512x512.png" 1024="$SET/icon_512x512@2x.png"
iconutil -c icns "$SET" -o AppIcon.icns
echo "Wrote assets/icon/AppIcon.icns"
