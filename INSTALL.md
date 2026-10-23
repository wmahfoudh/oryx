# Installing Oryx

Every package is on the [releases page](https://github.com/wmahfoudh/oryx/releases). This page gives the details for each system. On Linux, the command is `oryx`. On Windows and macOS, the installers do not add a terminal command (for the moment): [From a terminal](#from-a-terminal) says how to call Oryx there.

## Windows

There are four ways to install Oryx on Windows:

- **Microsoft Store**: search for *Oryx Editor*, or open [its page](https://apps.microsoft.com/detail/9NQGHNSJF3VB). The Store keeps it up to date.
- **winget**: `winget install Steerania.Oryx` in a terminal. winget downloads the MSI installer and runs it. `winget upgrade Steerania.Oryx` updates it. A new version reaches winget some days after its release.
- **MSI installer**: `oryx-<version>-windows-x86_64.msi`, a classic installer.
- **Zip**: `oryx-<version>-windows-x86_64.zip`, for an install in your user folder without administrator rights. Unzip it and run `install.ps1` in PowerShell. It copies Oryx to `%LOCALAPPDATA%\Programs\Oryx` and registers the file types. `install.ps1 -Uninstall` removes it.

The Store app is named *Oryx Editor* because the name *Oryx* was already taken there. It shows under that name in the Start menu.

## macOS

There are two ways to install Oryx on a Mac. Both give the same app, built for Apple Silicon and Intel Macs:

- **Homebrew**: `brew install --cask wmahfoudh/tap/oryx`
- **Disk image**: `oryx-<version>-macos-universal.dmg`. Open it and drag Oryx to Applications.

The app is not signed with an Apple developer account, so macOS blocks it the first time you open it. To allow it, go to System Settings, then Privacy and Security, and click **Open Anyway**. You only do this once. On older macOS versions, right-click the app and choose **Open**.

## Linux

### Packages

| Distribution | File | Command |
|---|---|---|
| Debian, Ubuntu | `oryx-editor_<version>_amd64.deb` | `sudo apt install ./oryx-editor_*_amd64.deb` |
| Fedora | `oryx-editor-<version>-1.x86_64.rpm` | `sudo dnf install ./oryx-editor-*.x86_64.rpm` |
| openSUSE | `oryx-editor-<version>-1.x86_64.rpm` | `sudo zypper install ./oryx-editor-*.x86_64.rpm` |
| Arch Linux | `oryx-editor-bin-<version>-1-x86_64.pkg.tar.zst` | `sudo pacman -U oryx-editor-bin-*.pkg.tar.zst` |

A package registers Oryx with your file manager, so markdown files and books open with it right away.

The packages are named `oryx-editor` because Arch Linux already has an unrelated package called `oryx`.

### AppImage

`Oryx-<version>-x86_64.AppImage` runs on most distributions without installing anything. Make it executable and run it:

```sh
chmod +x Oryx-*-x86_64.AppImage
./Oryx-*-x86_64.AppImage
```

You can update it in place with AppImageUpdate, AppImageLauncher or Gear Lever.

### Tarball

`oryx-<version>-linux-x86_64.tar.gz` installs Oryx in your home folder, without root:

```sh
tar -xzf oryx-*-linux-x86_64.tar.gz
cd oryx
./install.sh
```

It puts the binary in `~/.local/bin` and the themes and examples in `~/.local/share/oryx`, and registers the file types. `./install.sh --uninstall` removes it.

If you later switch from the tarball or from `make install` to a package, run `./install.sh --uninstall` first (in the repository, the script is `packaging/install.sh`). Otherwise the copy in your home folder comes first on the `PATH` and in the launcher, and you keep running the old one.

### Requirements

The Linux packages, the AppImage and the tarball need glibc 2.35 and OpenSSL 3. That means Debian 12, Ubuntu 22.04, Fedora 36, openSUSE Leap 15.6, or newer.

Oryx uses OpenSSL to download remote images, such as the badges at the top of a README. On a system without OpenSSL 3, Oryx does not start.

On X11, Oryx also needs the keyboard library `libxkbcommon-x11`. The deb and the rpm install it, and the AppImage carries its own copy. With the tarball, install the package yourself: `libxkbcommon-x11-0` on Debian and Ubuntu, `libxkbcommon-x11` on Fedora and Arch. Without it, Oryx says so and does not start.

## Building from source

You need Rust 1.89 or later. If you use rustup, it installs the version named in `rust-toolchain.toml` by itself. On Linux, the build also needs a C compiler, `pkg-config` and the development files of OpenSSL:

- Debian and Ubuntu: `sudo apt-get install pkg-config libssl-dev`
- Fedora: `sudo dnf install pkgconf perl-FindBin perl-IPC-Cmd openssl-devel`
- Arch Linux: `sudo pacman -S pkgconf openssl`
- openSUSE: `sudo zypper in libopenssl-devel`

```sh
git clone https://github.com/wmahfoudh/oryx.git
cd oryx
make install
```

`make install` is made for Linux. It builds the release binary, puts it in `~/.local/bin`, copies the themes and examples to `~/.local/share/oryx`, and registers the file types.

`cargo build --release` works too. The binary then looks for the `themes/` folder next to itself, in `~/.local/share/oryx`, and in the current folder.

Use a release build. A debug build is much slower on large files.

## From a terminal

On Linux, the command is `oryx`. On Windows and macOS, the installers do not add a terminal command (for the moment), and Oryx is called by its full path:

| Install | Program |
|---|---|
| Windows, MSI | `C:\Program Files\Oryx\oryx.exe` |
| Windows, zip | `%LOCALAPPDATA%\Programs\Oryx\oryx.exe` |
| macOS | `/Applications/Oryx.app/Contents/MacOS/oryx` |

The Store app has no terminal command. On Windows, adding the folder of `oryx.exe` to your `PATH` gives you the `oryx` command. In PowerShell, a path with a space needs `&` in front: `& "C:\Program Files\Oryx\oryx.exe" notes.md`.

## File types

On Linux, the packages and the install script register Oryx for markdown and text files, books and comics. On Windows, the Store app and `install.ps1` register it for the same files and for code files, and the MSI registers only `.md`, `.markdown` and `.epub`. On macOS, the file types come with the app.

`oryx --register` registers the file types by hand. On Linux, it is needed if you built with `cargo` or moved the binary yourself. Under a package it changes nothing, and says so:

```sh
oryx --register
```

On Windows, the same option adds the file types that the MSI leaves out. In PowerShell:

```powershell
& "C:\Program Files\Oryx\oryx.exe" --register
```