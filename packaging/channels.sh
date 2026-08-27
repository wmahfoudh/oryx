#!/bin/sh
# Rewrites the recipes for a published release: the two AUR PKGBUILDs
# and their .SRCINFO, the winget manifests, and the Flathub manifest with
# its cargo-sources.json. Run after the tag is pushed and the release
# created with its files attached; running it twice for a version
# changes nothing the second time.
#
# Usage: channels.sh <version>
#
# Needs the tag v<version> on GitHub (its tarball is downloaded and
# hashed, since GitHub makes it), the MSI and the Linux tarball in
# release/ as make release left them and as they were attached, msiinfo
# (the ProductCode is read from the MSI), makepkg, python3 with aiohttp
# and toml, and flatpak-builder-tools cloned beside the repository. The
# winget manifests are checked against Microsoft's schemas when python3
# has jsonschema and yaml; otherwise the check is skipped with a note.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
VERSION=$1
if [ -z "$VERSION" ]; then
    echo "usage: channels.sh <version>" >&2
    exit 1
fi
tag="v$VERSION"
repo_url=https://github.com/wmahfoudh/oryx
work="$root/target/channels/$VERSION"
mkdir -p "$work"

refuse() {
    echo "channels.sh: $1" >&2
    exit 1
}
for tool in curl git sha256sum msiinfo makepkg python3; do
    command -v "$tool" >/dev/null 2>&1 || refuse "$tool is not installed"
done
generator="$root/../flatpak-builder-tools/cargo/flatpak-cargo-generator.py"
[ -f "$generator" ] || refuse "flatpak-builder-tools is not cloned beside the repository ($generator)"
python3 -c 'import aiohttp, toml' 2>/dev/null || refuse "python3 needs aiohttp and toml for the cargo generator"

git -C "$root" ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1 \
    || refuse "tag $tag is not on GitHub"
git -C "$root" rev-parse --verify --quiet "$tag^{commit}" >/dev/null \
    || refuse "tag $tag is not in this checkout (git fetch --tags)"
msi="$root/release/oryx-$VERSION-windows-x86_64.msi"
linux="$root/release/oryx-$VERSION-linux-x86_64.tar.gz"
for file in "$msi" "$linux"; do
    [ -f "$file" ] || refuse "$file is missing (make release, then attach the files)"
done

# The hashes: the tag tarball as GitHub serves it, the release files as
# attached, the raw files the -bin package fetches from the tag.
tarball="$work/$tag.tar.gz"
curl -fsSL -o "$tarball" "$repo_url/archive/refs/tags/$tag.tar.gz" \
    || refuse "cannot download $repo_url/archive/refs/tags/$tag.tar.gz"
tag_sha=$(sha256sum "$tarball" | cut -d' ' -f1)
msi_sha=$(sha256sum "$msi" | cut -d' ' -f1 | tr 'a-f' 'A-F')
linux_sha=$(sha256sum "$linux" | cut -d' ' -f1)
product_code=$(msiinfo export "$msi" Property | tr -d '\r' | awk -F'\t' '$1 == "ProductCode" { print $2 }')
[ -n "$product_code" ] || refuse "no ProductCode in $msi"
release_date=$(git -C "$root" log -1 --format=%cs "$tag")
raw_sha() {
    if git -C "$root" cat-file -e "$tag:$1" 2>/dev/null; then
        git -C "$root" show "$tag:$1" | sha256sum | cut -d' ' -f1
    else
        echo "channels.sh: $1 is not in $tag, its checksum stays SKIP" >&2
        echo SKIP
    fi
}
desktop_sha=$(raw_sha packaging/linux/com.steerania.Oryx.desktop)
metainfo_sha=$(raw_sha packaging/linux/com.steerania.Oryx.metainfo.xml)
svg_sha=$(raw_sha assets/icon/oryx.svg)
stage_sha=$(raw_sha packaging/stage-linux.sh)

# The AUR: the version, a fresh pkgrel, the checksums in source order,
# then .SRCINFO from makepkg.
set_sums() {
    file=$1
    shift
    awk -v sums="$*" '
        BEGIN { n = split(sums, s, " ") }
        /^sha256sums=\(/ {
            printf "sha256sums=(\x27%s\x27", s[1]
            for (i = 2; i <= n; i++) printf "\n            \x27%s\x27", s[i]
            print ")"
            skipping = ($0 !~ /\)$/)
            next
        }
        skipping { if ($0 ~ /\)$/) skipping = 0; next }
        { print }
    ' "$file" > "$file.new"
    mv "$file.new" "$file"
}
for name in oryx-editor oryx-editor-bin; do
    dir="$here/aur/$name"
    sed -i "s/^pkgver=.*/pkgver=$VERSION/; s/^pkgrel=.*/pkgrel=1/" "$dir/PKGBUILD"
done
set_sums "$here/aur/oryx-editor/PKGBUILD" "$tag_sha"
set_sums "$here/aur/oryx-editor-bin/PKGBUILD" "$linux_sha" "$desktop_sha" "$metainfo_sha" "$svg_sha" "$stage_sha"
for name in oryx-editor oryx-editor-bin; do
    (cd "$here/aur/$name" && makepkg --printsrcinfo > .SRCINFO)
done

# winget: the three manifests filled from the templates, laid out as the
# winget-pkgs repository wants them.
out="$here/winget/manifests/s/Steerania/Oryx/$VERSION"
mkdir -p "$out"
fill() {
    sed -e "s|@VERSION@|$VERSION|g" \
        -e "s|@SHA256@|$msi_sha|g" \
        -e "s|@PRODUCT_CODE@|$product_code|g" \
        -e "s|@RELEASE_DATE@|$release_date|g" "$1" > "$2"
}
fill "$here/winget/Steerania.Oryx.installer.yaml" "$out/Steerania.Oryx.installer.yaml"
fill "$here/winget/Steerania.Oryx.locale.en-US.yaml" "$out/Steerania.Oryx.locale.en-US.yaml"
fill "$here/winget/Steerania.Oryx.version.yaml" "$out/Steerania.Oryx.yaml"
if python3 -c 'import jsonschema, yaml' 2>/dev/null; then
    for kind in installer defaultLocale version; do
        curl -fsSL -o "$work/$kind.schema.json" "https://aka.ms/winget-manifest.$kind.1.28.0.schema.json" \
            || refuse "cannot download the winget $kind schema"
    done
    python3 - "$out" "$work" <<'PY'
import json, sys, yaml, jsonschema
out, work = sys.argv[1], sys.argv[2]

# winget reads a bare date (ReleaseDate: 2026-08-23) as a string; PyYAML
# would make a date object of it and fail the schema's string type.
class Loader(yaml.SafeLoader):
    pass

Loader.add_constructor("tag:yaml.org,2002:timestamp", lambda loader, node: loader.construct_scalar(node))
pairs = [("Steerania.Oryx.installer.yaml", "installer"),
         ("Steerania.Oryx.locale.en-US.yaml", "defaultLocale"),
         ("Steerania.Oryx.yaml", "version")]
for name, kind in pairs:
    schema = json.load(open(f"{work}/{kind}.schema.json"))
    data = yaml.load(open(f"{out}/{name}"), Loader=Loader)
    jsonschema.validate(data, schema, format_checker=jsonschema.FormatChecker())
    print(f"{name}: valid against the {kind} 1.28.0 schema")
PY
else
    echo "channels.sh: winget schema check skipped, python3 has no jsonschema or yaml"
fi

# Flathub: the tag and its checksum, and the crate list for the offline
# build regenerated from Cargo.lock.
manifest="$here/flathub/com.steerania.Oryx.yml"
sed -i "s|archive/refs/tags/v[0-9.]*\.tar\.gz|archive/refs/tags/$tag.tar.gz|; s|^        sha256: .*|        sha256: \"$tag_sha\"|" "$manifest"
python3 "$generator" "$root/Cargo.lock" -o "$here/flathub/cargo-sources.json" >/dev/null

echo "channels.sh: recipes written for $VERSION"
echo "  tag tarball  $tag_sha"
echo "  MSI          $msi_sha  ProductCode $product_code  ReleaseDate $release_date"
echo "  Linux tarball $linux_sha"
git -C "$root" status --short packaging/
