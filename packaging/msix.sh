#!/bin/sh
# Builds the Store MSIX from the staged Windows release folder with
# makemsix, Microsoft's msix-packaging SDK built with pack support.
# Usage: msix.sh <version> <staging dir> <output.msix>
#
# The identity comes from packaging/msix/identity, the values Partner
# Center assigned when the name was reserved; the Store signs the package
# it receives against them, so the file is uploaded there and never
# attached to a release. The manifest is packaging/msix/AppxManifest.xml
# with the version and the identity filled in; the version takes a
# fourth part, 0, which the Store reserves. The logos are rasterized from
# the icon SVG at the sizes the manifest names, the wide tile with the
# mark centered on a transparent page. makemsix validates the manifest
# against the schemas as it packs.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
VERSION=$1
STAGE=$2
OUT=$3
if [ -z "$VERSION" ] || [ -z "$STAGE" ] || [ -z "$OUT" ]; then
    echo "usage: msix.sh <version> <staging dir> <output.msix>" >&2
    exit 1
fi
for tool in makemsix rsvg-convert; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "msix.sh: $tool is not installed" >&2
        exit 1
    fi
done
identity="$here/msix/identity"
template="$here/msix/AppxManifest.xml"
svg="$here/../assets/icon/oryx.svg"
for file in "$STAGE/oryx.exe" "$STAGE/LICENSE" "$STAGE/themes" "$STAGE/examples" "$identity" "$template" "$svg"; do
    if [ ! -e "$file" ]; then
        echo "msix.sh: $file is missing" >&2
        exit 1
    fi
done

value() {
    sed -n "s/^$1=//p" "$identity"
}
NAME=$(value Name)
PUBLISHER=$(value Publisher)
PUBLISHER_DISPLAY_NAME=$(value PublisherDisplayName)
DISPLAY_NAME=$(value DisplayName)
for pair in "Name=$NAME" "Publisher=$PUBLISHER" "PublisherDisplayName=$PUBLISHER_DISPLAY_NAME" "DisplayName=$DISPLAY_NAME"; do
    case "$pair" in
        *=) echo "msix.sh: ${pair%=} is missing from $identity" >&2; exit 1 ;;
    esac
done

pkg=$(mktemp -d)
trap 'rm -rf "$pkg"' EXIT
cp "$STAGE/oryx.exe" "$STAGE/LICENSE" "$pkg/"
cp -r "$STAGE/themes" "$pkg/themes"
cp -r "$STAGE/examples" "$pkg/examples"
mkdir "$pkg/Assets"
rsvg-convert -w 50 -h 50 "$svg" -o "$pkg/Assets/StoreLogo.png"
for size in 44 71 150 310; do
    rsvg-convert -w "$size" -h "$size" "$svg" -o "$pkg/Assets/Square${size}x${size}Logo.png"
done
rsvg-convert -w 150 -h 150 --page-width 310 --page-height 150 --left 80 "$svg" -o "$pkg/Assets/Wide310x150Logo.png"
sed -e "s|@VERSION@|$VERSION.0|g" \
    -e "s|@NAME@|$NAME|g" \
    -e "s|@PUBLISHER@|$PUBLISHER|g" \
    -e "s|@PUBLISHER_DISPLAY_NAME@|$PUBLISHER_DISPLAY_NAME|g" \
    -e "s|@DISPLAY_NAME@|$DISPLAY_NAME|g" \
    "$template" > "$pkg/AppxManifest.xml"

rm -f "$OUT"
makemsix pack -d "$pkg" -p "$OUT"
echo "$OUT: $(($(stat -c %s "$OUT") / 1024)) KB"
