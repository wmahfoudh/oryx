#!/bin/sh
# Packs the staged Linux tree as an AppImage with appimagetool.
# Usage: appimage.sh <staged usr dir> <output.AppImage>
#
# The AppDir holds the tree under usr/, AppRun from packaging/linux/,
# and at the top, where the runtime and the desktop integration tools
# look for them, the desktop entry and the 256 px icon with .DirIcon
# linked to it. The binary is the portable one build-linux.sh produces
# and nothing else is bundled: the image needs the system's glibc 2.35
# and libssl 3, the same floor as the tarball.
#
# appimagetool downloads the AppImage runtime from GitHub on every run
# unless given one. A copy kept at ~/.local/lib/appimagetool/runtime-x86_64
# (or the file APPIMAGE_RUNTIME names) is used when present, so a release
# builds without the network and every build embeds the same runtime.
# The tool's own AppStream check is skipped (-n): it only recognizes the
# old .appdata.xml name, and tests/packaging.rs validates the metainfo.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
STAGE=$1
OUT=$2
if [ -z "$STAGE" ] || [ -z "$OUT" ]; then
    echo "usage: appimage.sh <staged usr dir> <output.AppImage>" >&2
    exit 1
fi
if ! command -v appimagetool >/dev/null 2>&1; then
    echo "appimage.sh: appimagetool is not installed" >&2
    exit 1
fi
app=com.steerania.Oryx
apprun="$here/linux/AppRun"
entry="$STAGE/share/applications/$app.desktop"
icon="$STAGE/share/icons/hicolor/256x256/apps/$app.png"
for file in "$STAGE/bin/oryx" "$entry" "$icon" "$apprun"; do
    if [ ! -e "$file" ]; then
        echo "appimage.sh: $file is missing" >&2
        exit 1
    fi
done

appdir=$(mktemp -d)
trap 'rm -rf "$appdir"' EXIT
cp -r "$STAGE" "$appdir/usr"
install -m755 "$apprun" "$appdir/AppRun"
cp "$entry" "$appdir/$app.desktop"
cp "$icon" "$appdir/$app.png"
ln -s "$app.png" "$appdir/.DirIcon"

runtime="${APPIMAGE_RUNTIME:-$HOME/.local/lib/appimagetool/runtime-x86_64}"
rm -f "$OUT"
if [ -f "$runtime" ]; then
    ARCH=x86_64 appimagetool -n --runtime-file "$runtime" "$appdir" "$OUT"
else
    echo "appimage.sh: no runtime at $runtime, appimagetool downloads one" >&2
    ARCH=x86_64 appimagetool -n "$appdir" "$OUT"
fi
echo "$OUT: $(($(stat -c %s "$OUT") / 1024)) KB"
