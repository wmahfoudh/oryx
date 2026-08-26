#!/bin/sh
# Creates the Ubuntu 22.04 root the Linux release is built in,
# /var/lib/oryx-jammy (not /var/lib/machines, a btrfs subvolume stub that
# refuses writes on this machine). Run once, with sudo. Building there fixes
# the floor the shipped binary needs at glibc 2.35 and libssl 3, which
# covers Debian 12, Ubuntu 22.04, Fedora 36, openSUSE Leap 15.4 and newer.
#
# Inside: build-essential, libssl-dev, pkg-config, curl, ca-certificates,
# git, and a build user carrying the invoking user's uid with a rustup
# stable toolchain, so build-linux.sh can enter the root through
# bubblewrap without root and write as that user. Every step checks what
# is already there, so an interrupted run resumes with the same command.
set -e
ROOT=/var/lib/oryx-jammy
MIRROR=http://archive.ubuntu.com/ubuntu
KEYRING=/usr/share/keyrings/ubuntu-archive-keyring.gpg
BUILD_UID=${SUDO_UID:-1000}

if [ "$(id -u)" -ne 0 ]; then
    echo "chroot.sh: run with sudo" >&2
    exit 1
fi
if [ ! -f "$KEYRING" ]; then
    echo "chroot.sh: $KEYRING is missing (the ubuntu-keyring package)" >&2
    exit 1
fi
for tool in debootstrap chroot; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "chroot.sh: $tool is not installed" >&2
        exit 1
    fi
done

if [ ! -f "$ROOT/etc/os-release" ]; then
    debootstrap --variant=minbase --arch=amd64 --keyring="$KEYRING" jammy "$ROOT" "$MIRROR"
fi
cp /etc/resolv.conf "$ROOT/etc/resolv.conf"
# The rustup installer and its cargo launcher read /proc/self/exe.
mount -t proc proc "$ROOT/proc"
trap 'umount "$ROOT/proc"' EXIT
# passwd brings useradd, which minbase leaves out.
chroot "$ROOT" /bin/sh -c '
set -e
export DEBIAN_FRONTEND=noninteractive LC_ALL=C.UTF-8
apt-get update
apt-get install -y --no-install-recommends build-essential libssl-dev pkg-config curl ca-certificates git passwd
apt-get clean
rm -rf /var/lib/apt/lists/*
'
if ! chroot "$ROOT" getent passwd build >/dev/null; then
    # By full path: the host's sudo PATH has no /usr/sbin.
    chroot "$ROOT" /usr/sbin/useradd --uid "$BUILD_UID" --user-group --create-home --shell /bin/bash build
fi
if [ ! -x "$ROOT/home/build/.cargo/bin/cargo" ]; then
    # The installer also reads the login shell.
    chroot --userspec="$BUILD_UID:$BUILD_UID" "$ROOT" /bin/sh -c '
    set -e
    export HOME=/home/build SHELL=/bin/bash
    cd "$HOME"
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path
    '
fi
chroot --userspec="$BUILD_UID:$BUILD_UID" "$ROOT" /bin/sh -c '
export HOME=/home/build PATH=/home/build/.cargo/bin:$PATH
cargo --version
rustc --version
'
echo "chroot.sh: $ROOT is ready"
