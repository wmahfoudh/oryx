# Installs or removes Oryx for the current user.
param([switch]$Uninstall)
$dest = Join-Path $env:LOCALAPPDATA "Programs\Oryx"
if ($Uninstall) {
    Remove-Item -Recurse -Force $dest -ErrorAction SilentlyContinue
    Write-Output "oryx removed; registry associations stay until overwritten"
    exit 0
}
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item (Join-Path $PSScriptRoot "oryx.exe") $dest -Force
Copy-Item (Join-Path $PSScriptRoot "themes") $dest -Recurse -Force
Copy-Item (Join-Path $PSScriptRoot "examples") $dest -Recurse -Force
& (Join-Path $dest "oryx.exe") --register
Write-Output "installed to $dest"
