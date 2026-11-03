$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$errors = $null
$tokens = $null
[void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $root 'install.ps1'), [ref]$tokens, [ref]$errors)
if ($errors -and $errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Output $_.ToString() }
    throw 'parse failed'
}

$env:CURSOR_NOTIFY_SELFTEST = '1'
& (Join-Path $root 'progress-host.ps1')
Remove-Item Env:CURSOR_NOTIFY_SELFTEST

$fakeHome = Join-Path $env:TEMP ('cursor-done-notify-osstest-' + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Force -Path (Join-Path $fakeHome '.cursor') | Out-Null
$hooks = Join-Path $fakeHome '.cursor\hooks.json'
$relay = Join-Path $root 'hook-relay.exe'
$foreign = 'C:\keep\other-hook.cmd'
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($hooks, (@{
    version = 1
    hooks = @{
        stop = @(
            @{ command = $foreign; timeout = 10 }
            @{ command = "$relay empty"; timeout = 5 }
        )
        preToolUse = @(
            @{ command = "$relay allow"; timeout = 5 }
        )
    }
} | ConvertTo-Json -Depth 6), $utf8)

function Get-Commands($Name) {
    $parsed = Get-Content -LiteralPath $hooks -Raw -Encoding UTF8 | ConvertFrom-Json
    if (-not $parsed.hooks.PSObject.Properties.Name -contains $Name) {
        return @()
    }
    return @(@($parsed.hooks.$Name) | ForEach-Object { [string]$_.command })
}

$env:USERPROFILE = $fakeHome
$env:CURSOR_NOTIFY_SKIP_STOP = '1'
if (Test-Path -LiteralPath $relay) {
    $env:CURSOR_NOTIFY_SKIP_BUILD = '1'
}
try {
    & (Join-Path $root 'install.ps1')
    $stop = Get-Commands 'stop'
    $pre = Get-Commands 'preToolUse'
    if (@($stop | Where-Object { $_ -eq $foreign }).Count -ne 1) { throw "install dropped the other hook: $($stop -join ' | ')" }
    if (@($stop | Where-Object { $_ -like '*hook-relay.exe empty' }).Count -ne 1) { throw "install stop relay: $($stop -join ' | ')" }
    if (@($pre | Where-Object { $_ -like '*hook-relay.exe allow' }).Count -ne 1) { throw "install preToolUse relay: $($pre -join ' | ')" }

    & (Join-Path $root 'install.ps1') -Uninstall
    $stop = Get-Commands 'stop'
    $pre = Get-Commands 'preToolUse'
    if (@($stop | Where-Object { $_ -eq $foreign }).Count -ne 1) { throw "uninstall dropped the other hook: $($stop -join ' | ')" }
    if (@($stop + $pre | Where-Object { $_ -like '*hook-relay*' }).Count -ne 0) { throw 'uninstall left a relay hook' }
} finally {
    if (Test-Path -LiteralPath $fakeHome) {
        Remove-Item -LiteralPath $fakeHome -Recurse -Force
    }
}

Write-Output 'INSTALL TEST OK'
