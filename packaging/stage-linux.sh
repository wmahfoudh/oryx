#!/bin/sh
# Lays a release folder out as the tree every Linux package installs:
# the .deb and the .rpm, the AppImage (AppDir/usr), the AUR packages
# ($pkgdir/usr) and the Flatpak (/app).
#
# Usage: stage-linux.sh <srcdir> <destroot>
#
# <srcdir> holds oryx, themes/, examples/ and LICENSE, as `make release`
# stages them; <destroot> is the install prefix (usr, or the Flatpak's
# app). The icons are rasterized from the SVG at the hicolor sizes, and
# the SVG itself is the scalable icon. Every source file is checked
# before anything is written, so a failure leaves no half tree.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
src="$1"
dest="$2"
if [ -z "$src" ] || [ -z "$dest" ]; then
    echo "usage: stage-linux.sh <srcdir> <destroot>" >&2
    exit 1
fi
app=com.steerania.Oryx
svg="$here/../assets/icon/oryx.svg"
entry="$here/linux/$app.desktop"
metainfo="$here/linux/$app.metainfo.xml"
for file in "$src/oryx" "$src/themes" "$src/examples" "$src/LICENSE" "$svg" "$entry" "$metainfo"; do
    if [ ! -e "$file" ]; then
        echo "stage-linux.sh: $file is missing" >&2
        exit 1
    fi
done
if ! command -v rsvg-convert >/dev/null 2>&1; then
    echo "stage-linux.sh: rsvg-convert is not installed" >&2
    exit 1
fi

install -Dm755 "$src/oryx" "$dest/bin/oryx"
mkdir -p "$dest/share/oryx"
rm -rf "$dest/share/oryx/themes" "$dest/share/oryx/examples"
cp -r "$src/themes" "$dest/share/oryx/themes"
cp -r "$src/examples" "$dest/share/oryx/examples"
find "$dest/share/oryx" -type d -exec chmod 755 {} +
find "$dest/share/oryx" -type f -exec chmod 644 {} +
install -Dm644 "$entry" "$dest/share/applications/$app.desktop"
install -Dm644 "$metainfo" "$dest/share/metainfo/$app.metainfo.xml"
for size in 16 32 48 64 128 256 512; do
    dir="$dest/share/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$dir"
    rsvg-convert -w "$size" -h "$size" "$svg" -o "$dir/$app.png"
done
install -Dm644 "$svg" "$dest/share/icons/hicolor/scalable/apps/$app.svg"
install -Dm644 "$src/LICENSE" "$dest/share/licenses/oryx-editor/LICENSE"
