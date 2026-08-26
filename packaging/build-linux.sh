#!/bin/sh
# Builds the Linux release binary inside the Ubuntu 22.04 root that
# chroot.sh created, so the shipped binary imports nothing past glibc 2.35
# and links the system's libssl 3. Bubblewrap enters the root as the
# invoking user: no root needed, the checkout bound at /home/build/src
# (a mount point bwrap can create, the home being the user's), the build
# written to target/jammy so the native target directory stays untouched.
#
# Usage: build-linux.sh                        builds target/jammy/release/oryx
#        build-linux.sh --check <binary> [max]  refuses a binary importing a
#                                              glibc version above max
#                                              (2.35 unless given)
set -e
ROOT=/var/lib/oryx-jammy
FLOOR=2.35

# The highest GLIBC_ symbol version a binary imports, against the floor.
check() {
    binary=$1
    max=${2:-$FLOOR}
    highest=$(objdump -T "$binary" | grep -oE 'GLIBC_[0-9.]+' | sed 's/GLIBC_//' | sort -V | tail -1)
    if [ -z "$highest" ]; then
        echo "build-linux.sh: $binary imports no glibc symbol" >&2
        exit 1
    fi
    if [ "$(printf '%s\n%s\n' "$max" "$highest" | sort -V | tail -1)" != "$max" ]; then
        echo "build-linux.sh: $binary imports GLIBC_$highest, above the $max floor" >&2
        exit 1
    fi
    echo "$binary: imports up to GLIBC_$highest, within the $max floor"
}

if [ "$1" = "--check" ]; then
    if [ -z "$2" ]; then
        echo "usage: build-linux.sh --check <binary> [max]" >&2
        exit 1
    fi
    check "$2" "$3"
    exit 0
fi

if [ ! -d "$ROOT" ]; then
    echo "build-linux.sh: $ROOT is missing; create it once with: sudo sh packaging/chroot.sh" >&2
    exit 1
fi
if ! command -v bwrap >/dev/null 2>&1; then
    echo "build-linux.sh: bubblewrap (bwrap) is not installed" >&2
    exit 1
fi
src="$(cd "$(dirname "$0")/.." && pwd)"
bwrap --bind "$ROOT" / \
      --dev /dev \
      --proc /proc \
      --tmpfs /tmp \
      --ro-bind /etc/resolv.conf /etc/resolv.conf \
      --bind "$src" /home/build/src \
      --chdir /home/build/src \
      --unshare-pid \
      --setenv HOME /home/build \
      --setenv PATH /home/build/.cargo/bin:/usr/local/bin:/usr/bin:/bin \
      --setenv CARGO_TARGET_DIR /home/build/src/target/jammy \
      cargo build --release --locked
binary="$src/target/jammy/release/oryx"
check "$binary"
echo "$binary needs: $(readelf -d "$binary" | awk '/NEEDED/ {gsub(/[][]/, "", $NF); printf "%s ", $NF}')"
