# Build the two programs and zip them with the sprites and the installer.
# Pass -Version with the release tag (v1.2.3); it must match AppInfo.Version in Host.cs.
param([string]$Version)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$hostSource = [System.IO.File]::ReadAllText((Join-Path $root 'Host.cs'))
$match = [regex]::Match($hostSource, 'public const string Version = "([^"]+)"')
if (-not $match.Success) {
    throw 'AppInfo.Version not found in Host.cs'
}
$current = $match.Groups[1].Value
if ($Version -and $Version.TrimStart('v', 'V') -ne $current) {
    throw "Tag $Version does not match AppInfo.Version $current in Host.cs"
}

$csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $csc)) {
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'
}

$name = "cursor-done-notify-v$current"
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name
Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null

& $csc /nologo /warn:4 /warnaserror /codepage:65001 /target:exe "/out:$(Join-Path $stage 'hook-relay.exe')" (Join-Path $root 'HookRelay.cs')
if ($LASTEXITCODE -ne 0) { throw 'hook-relay.exe build failed' }
$sources = @('BuddyForm.cs', 'Quota.cs', 'Host.cs', 'HostApp.cs') | ForEach-Object { Join-Path $root $_ }
& $csc /nologo /warn:4 /warnaserror /codepage:65001 /target:winexe "/out:$(Join-Path $stage 'buddy-host.exe')" /r:System.Drawing.dll /r:System.Windows.Forms.dll /r:System.Web.Extensions.dll $sources
if ($LASTEXITCODE -ne 0) { throw 'buddy-host.exe build failed' }

foreach ($file in @('install.ps1', 'install.cmd', 'uninstall.cmd', 'README.md', 'LICENSE')) {
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $stage
}
Copy-Item -LiteralPath (Join-Path $root 'sprites') -Destination $stage -Recurse

$zip = Join-Path $dist "$name.zip"
Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path $stage -DestinationPath $zip
Write-Output $zip
