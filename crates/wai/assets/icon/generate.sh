#!/bin/sh
# Renders icon.svg into the files cargo-packager uses (macOS `sips`). It builds the .icns from
# the pngs itself (`@2x` is the retina 512 slot); the NSIS installer needs the .ico.
set -eu
cd "$(dirname "$0")"
sips -s format png -z 512 512 icon.svg --out icon.png
sips -s format png -z 1024 1024 icon.svg --out icon@2x.png
sips -s format ico -z 256 256 icon.svg --out icon.ico
