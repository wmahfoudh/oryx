#!/bin/sh
# Builds the Windows MSI from the staged release folder with wixl
# (msitools). Usage: msi.sh <version> <staging dir> <output.msi>.
#
# The package installs per machine to Program Files, adds a Start menu
# shortcut and an Add/Remove entry, and lists Oryx under Open with for
# .md, .markdown and .epub through the same Oryx.Document ProgId that
# `oryx --register` writes per user; the full extension list stays with
# --register. The UpgradeCode is permanent: a newer MSI replaces the
# installed version in place.
set -e

VERSION=$1
STAGE=$2
OUT=$3
UPGRADE_CODE=8ea2ee23-91f8-46ec-9310-6dfbf39a04c9

WXS=$(mktemp --suffix=.wxs)
trap 'rm -f "$WXS"' EXIT

# One component per file, ids numbered in walk order. MSI ids must be
# plain identifiers, so file names never become ids.
component_id=0
refs=""

emit_file() {
    component_id=$((component_id + 1))
    name=$(basename "$1")
    keypath=""
    extra=""
    if [ "$name" = "oryx.exe" ]; then
        keypath=' KeyPath="yes"'
        extra='<Shortcut Id="StartMenuOryx" Directory="ProgramMenuFolder" Name="Oryx" Icon="oryx.exe" Advertise="yes"/>'
    fi
    printf '          <Component Id="C%s" Guid="*">\n' "$component_id"
    printf '            <File Id="F%s" Name="%s" Source="%s"%s>%s</File>\n' \
        "$component_id" "$name" "$1" "$keypath" "$extra"
    printf '          </Component>\n'
    refs="$refs C$component_id"
}

emit_dir() {
    for f in "$STAGE/$1"/*; do
        [ -f "$f" ] || { echo "msi.sh: unexpected nesting under $1" >&2; exit 1; }
        emit_file "$f"
    done
}

{
    cat <<HEAD
<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Product Id="*" Name="Oryx" Version="$VERSION" Manufacturer="Walid"
           Language="1033" UpgradeCode="$UPGRADE_CODE">
    <Package InstallerVersion="200" Compressed="yes"/>
    <Media Id="1" Cabinet="oryx.cab" EmbedCab="yes"/>
    <Property Id="ALLUSERS" Value="1"/>
    <MajorUpgrade DowngradeErrorMessage="A newer version of Oryx is already installed."/>
    <Icon Id="oryx.exe" SourceFile="$STAGE/oryx.exe"/>
    <Property Id="ARPPRODUCTICON" Value="oryx.exe"/>
    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="ProgramMenuFolder"/>
      <Directory Id="ProgramFiles64Folder">
        <Directory Id="INSTALLDIR" Name="Oryx">
HEAD
    for f in "$STAGE"/*; do
        # The zip's per-user install script has no place inside the MSI.
        [ "$(basename "$f")" = "install.ps1" ] && continue
        [ -f "$f" ] && emit_file "$f"
    done
    printf '          <Directory Id="ThemesDir" Name="themes">\n'
    emit_dir themes
    printf '          </Directory>\n'
    printf '          <Directory Id="ExamplesDir" Name="examples">\n'
    emit_dir examples
    printf '          </Directory>\n'
    cat <<ASSOC
        </Directory>
      </Directory>
      <Component Id="RegAssoc" Guid="*">
        <RegistryValue Root="HKLM" Key="Software\\Classes\\Oryx.Document" Type="string" Value="Oryx Document" KeyPath="yes"/>
        <RegistryValue Root="HKLM" Key="Software\\Classes\\Oryx.Document\\DefaultIcon" Type="string" Value="[INSTALLDIR]oryx.exe,0"/>
        <RegistryValue Root="HKLM" Key="Software\\Classes\\Oryx.Document\\shell\\open\\command" Type="string" Value="&quot;[INSTALLDIR]oryx.exe&quot; &quot;%1&quot;"/>
        <RegistryValue Root="HKLM" Key="Software\\Classes\\.md\\OpenWithProgids" Name="Oryx.Document" Type="string" Value=""/>
        <RegistryValue Root="HKLM" Key="Software\\Classes\\.markdown\\OpenWithProgids" Name="Oryx.Document" Type="string" Value=""/>
        <RegistryValue Root="HKLM" Key="Software\\Classes\\.epub\\OpenWithProgids" Name="Oryx.Document" Type="string" Value=""/>
      </Component>
    </Directory>
    <Feature Id="Main" Level="1">
ASSOC
    for ref in $refs; do
        printf '      <ComponentRef Id="%s"/>\n' "$ref"
    done
    cat <<TAIL
      <ComponentRef Id="RegAssoc"/>
    </Feature>
  </Product>
</Wix>
TAIL
} > "$WXS"

wixl -a x64 -o "$OUT" "$WXS"
