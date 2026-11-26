# Packages the Windows build of DiscCraft (.github/workflows/release.yml runs it):
#
#   ./packaging/windows/package.ps1 -Version 0.1.0-alpha.6 -Arch x64 -BinDir target/release
#
# Writes disccraft-<version>-windows-<arch>.msi (WiX Toolset 5, `wix` on the path) and
# disccraft-<version>-windows-<arch>-portable.zip (the programs and documents, nothing installed). Nothing is signed:
# SmartScreen asks before the installer first runs.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][ValidateSet('x64', 'arm64')][string]$Arch,
    [Parameter(Mandatory = $true)][string]$BinDir,
    [string]$Out = 'dist'
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$BinDir = (Resolve-Path $BinDir).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$docs = 'README.md', 'README.ja.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md'

# An installer's version is three numbers, each at most 65535, that a later release must raise: a pre-release N
# of a.b.c is a.b.(c*1000 + 100*stage + N) (alpha 1, beta 2, rc 3), the release itself a.b.(c*1000 + 999).
function Get-MsiVersion([string]$version) {
    if ($version -eq 'test') { return '0.0.1' }
    if ($version -notmatch '^(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc)\.(\d+))?$') {
        throw "version $version is not a.b.c or a.b.c-(alpha|beta|rc).N"
    }
    $patch = [int]$Matches[3] * 1000
    if ($Matches[4]) {
        $stage = @{ alpha = 1; beta = 2; rc = 3 }[$Matches[4]]
        $number = [int]$Matches[5]
        if ($number -gt 99) { throw "pre-release number $number is over 99" }
        $patch += 100 * $stage + $number
    } else {
        $patch += 999
    }
    if ($patch -gt 65535) { throw "version $version does not fit an installer's version" }
    return "$($Matches[1]).$($Matches[2]).$patch"
}
$msiVersion = Get-MsiVersion $Version
Write-Host "installer version $msiVersion"

# The portable zip.
$base = "disccraft-$Version-windows-$Arch"
$work = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid())
$folder = Join-Path $work "$base-portable"
New-Item -ItemType Directory -Force -Path $folder | Out-Null
Copy-Item (Join-Path $BinDir 'disccraft.exe'), (Join-Path $BinDir 'disccraft-cli.exe') $folder
foreach ($doc in $docs) { Copy-Item (Join-Path $root $doc) $folder }
Compress-Archive -Path $folder -DestinationPath (Join-Path $Out "$base-portable.zip") -Force

# The installer.
wix build (Join-Path $PSScriptRoot 'disccraft.wxs') -arch $Arch `
    -d "BinDir=$BinDir" -d "RootDir=$root" -d "MsiVersion=$msiVersion" `
    -o (Join-Path $Out "$base.msi")
if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }
Remove-Item (Join-Path $Out "$base.wixpdb") -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $work
Get-ChildItem $Out
