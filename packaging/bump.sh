#!/bin/sh
# Opens a phase at a new version: the crate version and its lock, a
# release line in the metainfo marked development (the release routine
# sets its date and removes the mark), the metainfo's screenshot links
# on the new tag, and the changelog heading. The version moves once per
# phase, so a bump while the newest release line is still marked
# development is refused, as is any version not above the crate's.
#
# Usage: bump.sh <X.Y.Z> [root]
set -e
here="$(cd "$(dirname "$0")" && pwd)"
root="${2:-$(cd "$here/.." && pwd)}"
VERSION=$1

refuse() {
    echo "bump.sh: $1" >&2
    exit 1
}
echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || refuse "usage: bump.sh <X.Y.Z> [root]"
manifest="$root/Cargo.toml"
metainfo="$root/packaging/linux/com.steerania.Oryx.metainfo.xml"
changelog="$root/CHANGELOG.md"
for file in "$manifest" "$metainfo" "$changelog"; do
    [ -f "$file" ] || refuse "$file is missing"
done
current=$(sed -n 's/^version = "\(.*\)"/\1/p' "$manifest" | head -1)
[ -n "$current" ] || refuse "no version line in $manifest"
awk -v a="$VERSION" -v b="$current" 'BEGIN {
    split(a, x, "."); split(b, y, ".")
    for (i = 1; i <= 3; i++) {
        if (x[i] + 0 > y[i] + 0) exit 0
        if (x[i] + 0 < y[i] + 0) exit 1
    }
    exit 1
}' || refuse "$VERSION is not above the crate's $current"
newest=$(grep -m1 '<release ' "$metainfo")
case "$newest" in
    *'type="development"'*) refuse "the newest release line is still marked development: a phase is open, and the version moves once per phase" ;;
esac
today=$(date +%F)

# The crate: the first version line is the package's own.
sed -i "0,/^version = \".*\"/s//version = \"$VERSION\"/" "$manifest"
cargo update --workspace --offline --quiet --manifest-path "$manifest"

# The metainfo: the new line above the newest, at its indent, and the
# screenshot links on the tag the release will push.
sed -i "0,/^\( *\)<release /s//\1<release version=\"$VERSION\" date=\"$today\" type=\"development\"\/>\n\1<release /" "$metainfo"
sed -i "s|/wmahfoudh/oryx/v[0-9.]*/screenshots/|/wmahfoudh/oryx/v$VERSION/screenshots/|" "$metainfo"

# The changelog: the heading on top, unless it is already there.
grep -q "^## v$VERSION\$" "$changelog" || sed -i "0,/^## /s//## v$VERSION\n\n## /" "$changelog"

echo "bump.sh: $current to $VERSION"
git -C "$root" status --short Cargo.toml Cargo.lock packaging/linux CHANGELOG.md 2>/dev/null || true
