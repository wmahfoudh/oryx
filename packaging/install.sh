#!/bin/sh
# Installs or removes Oryx for the current user.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
bin="$HOME/.local/bin"
data="$HOME/.local/share/oryx"

if [ "$1" = "--uninstall" ]; then
    rm -f "$bin/oryx"
    rm -rf "$data"
    rm -f "$HOME/.local/share/applications/oryx.desktop"
    for size in 16 32 48 64 128 256; do
        rm -f "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/oryx.png"
    done
    echo "oryx removed"
    exit 0
fi

mkdir -p "$bin" "$data"
install -m 755 "$here/oryx" "$bin/oryx"
rm -rf "$data/themes" "$data/examples"
cp -r "$here/themes" "$data/themes"
cp -r "$here/examples" "$data/examples"
"$bin/oryx" --register
echo "installed to $bin/oryx; themes in $data/themes; examples in $data/examples"
case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "note: $bin is not on your PATH" ;;
esac
