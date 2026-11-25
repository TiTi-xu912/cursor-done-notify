$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$errors = $null
$tokens = $null
[void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $root 'install.ps1'), [ref]$tokens, [ref]$errors)
if ($errors -and $errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Output $_.ToString() }
    throw 'parse failed'
}

& (Join-Path $PSScriptRoot 'host.ps1')

$originalTemp = $env:TEMP
$fakeHome = Join-Path $originalTemp ('cursor-done-notify-osstest-' + [guid]::NewGuid().ToString('n'))
$installRoot = Join-Path $fakeHome 'install folder with spaces'
New-Item -ItemType Directory -Force -Path (Join-Path $fakeHome '.cursor'), $installRoot | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'install.ps1') -Destination $installRoot
Copy-Item -LiteralPath (Join-Path $root 'HookRelay.cs') -Destination $installRoot

$csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $csc)) {
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'
}
$relay = Join-Path $installRoot 'hook-relay.exe'
& $csc /nologo /warn:4 /codepage:65001 /target:exe "/out:$relay" (Join-Path $installRoot 'HookRelay.cs')
if ($LASTEXITCODE -ne 0) { throw 'test relay compile failed' }

$savedTemp = $env:TEMP
$savedTmp = $env:TMP
$env:TEMP = $fakeHome
$env:TMP = $fakeHome
try {
    $reply = '{"hook_event_name":"beforeSubmitPrompt","conversation_id":"t","prompt":"hi"}' | & $relay continue
    if ($reply -ne '{"continue":true}') { throw "relay reply: $reply" }
    '{"hook_event_name":"stop","conversation_id":"t","status":"completed"}' | & $relay empty | Out-Null
} finally {
    $env:TEMP = $savedTemp
    $env:TMP = $savedTmp
}
$inbox = Join-Path $fakeHome 'cursor-done-notify\inbox'
$events = @(Get-ChildItem -LiteralPath $inbox -Filter '*.json' | Sort-Object Name | ForEach-Object {
    Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
})
if ($events.Count -ne 2) { throw "relay wrote $($events.Count) events" }
if ($events[0].prompt -ne 'hi' -or $events[0].conversation_id -ne 't') { throw 'relay changed the prompt event' }
if ($events[0].host_window -and -not $events[0].host_process) { throw 'host window without its process name' }
if ($events[1].PSObject.Properties.Name -contains 'host_window') { throw 'only the first event of a turn records the host window' }
Remove-Item -LiteralPath (Join-Path $fakeHome 'cursor-done-notify') -Recurse -Force

if (-not ('CursorDoneNotify.PathApi' -as [type])) {
    Add-Type -Namespace CursorDoneNotify -Name PathApi -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint bufferLength);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern uint GetShortPathName(string longPath, System.Text.StringBuilder shortPath, uint bufferLength);
'@
}
$buffer = New-Object System.Text.StringBuilder 1024
[void][CursorDoneNotify.PathApi]::GetLongPathName($relay, $buffer, [uint32]$buffer.Capacity)
$relayLong = $buffer.ToString()
$buffer = New-Object System.Text.StringBuilder 1024
[void][CursorDoneNotify.PathApi]::GetShortPathName($relay, $buffer, [uint32]$buffer.Capacity)
$relayShort = $buffer.ToString()
if (-not $relayLong -or -not $relayShort) { throw 'could not resolve relay path forms' }

function Test-RelayCommand([string]$Command) {
    foreach ($form in @($relay, $relayLong, $relayShort)) {
        if ($Command.IndexOf($form, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) { return $true }
    }
    return $false
}

$hooks = Join-Path $fakeHome '.cursor\hooks.json'
$foreign = 'C:\keep\hook-relay.exe empty'
$other = 'C:\keep\other-hook.cmd'
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($hooks, (@{
    version = 1
    futureTopLevel = 'keep-me'
    hooks = @{
        stop = @(
            @{ command = $foreign; timeout = 10; futureField = 'keep-me' }
            @{ command = "$relay empty"; timeout = 5 }
            @{ command = "$relayShort empty"; timeout = 5 }
            @{ command = "& '$relayLong' empty"; timeout = 5 }
        )
        preToolUse = @(
            @{ command = "$relay allow"; timeout = 5 }
        )
        postToolUse = @(
            @{ command = $other; timeout = 9; matcher = 'Read' }
        )
        beforeSubmitPrompt = @(
            @{ type = 'prompt'; prompt = 'Keep this policy'; timeout = 7 }
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
$env:TEMP = $fakeHome
$env:CURSOR_NOTIFY_SKIP_STOP = '1'
$env:CURSOR_NOTIFY_SKIP_BUILD = '1'
$installer = Join-Path $installRoot 'install.ps1'
try {
    & $installer
    $stop = Get-Commands 'stop'
    $pre = Get-Commands 'preToolUse'
    $expected = "& '$relayLong' empty"
    if (@($stop | Where-Object { $_ -eq $foreign }).Count -ne 1) { throw "install dropped a same-named foreign hook: $($stop -join ' | ')" }
    if (@($stop | Where-Object { $_ -eq $expected }).Count -ne 1) { throw "install did not quote a spaced path: $($stop -join ' | ')" }
    if (@($stop | Where-Object { Test-RelayCommand $_ }).Count -ne 1) { throw "install left another spelling of its own hook: $($stop -join ' | ')" }
    if (@($pre | Where-Object { Test-RelayCommand $_ }).Count -ne 0) { throw 'safe install kept a permission hook' }
    $parsed = Get-Content -LiteralPath $hooks -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($parsed.futureTopLevel -ne 'keep-me') { throw 'install dropped an unknown top-level field' }
    $foreignItem = @($parsed.hooks.stop | Where-Object { $_.command -eq $foreign })[0]
    if ($foreignItem.futureField -ne 'keep-me') { throw 'install dropped an unknown hook field' }
    $otherItem = @($parsed.hooks.postToolUse | Where-Object { $_.command -eq $other })[0]
    if ($otherItem.matcher -ne 'Read') { throw 'install dropped another hook matcher' }
    $promptItem = @($parsed.hooks.beforeSubmitPrompt | Where-Object { $_.type -eq 'prompt' })[0]
    if ($promptItem.prompt -ne 'Keep this policy') { throw 'install dropped a prompt hook without a command' }
    if (-not (Test-Path -LiteralPath "$hooks.bak")) { throw 'install did not create a hooks backup' }

    & $installer -EnablePermissionHooks
    $pre = Get-Commands 'preToolUse'
    if (@($pre | Where-Object { $_ -eq "& '$relayLong' allow" }).Count -ne 1) { throw 'opt-in permission hook was not installed' }

    & $installer -Uninstall
    $stop = Get-Commands 'stop'
    $pre = Get-Commands 'preToolUse'
    if (@($stop | Where-Object { $_ -eq $foreign }).Count -ne 1) { throw "uninstall dropped the other hook: $($stop -join ' | ')" }
    if (@($stop + $pre | Where-Object { Test-RelayCommand $_ }).Count -ne 0) { throw 'uninstall left an owned relay hook' }
    $parsed = Get-Content -LiteralPath $hooks -Raw -Encoding UTF8 | ConvertFrom-Json
    $promptItem = @($parsed.hooks.beforeSubmitPrompt | Where-Object { $_.type -eq 'prompt' })[0]
    if ($promptItem.prompt -ne 'Keep this policy') { throw 'uninstall dropped a prompt hook without a command' }

    [System.IO.File]::WriteAllText($hooks, '{broken', $utf8)
    $failedSafely = $false
    try {
        & $installer
    } catch {
        $failedSafely = $true
    }
    if (-not $failedSafely) { throw 'install accepted invalid existing JSON' }
    if ((Get-Content -LiteralPath $hooks -Raw) -ne '{broken') { throw 'install overwrote invalid existing JSON' }
} finally {
    $env:TEMP = $originalTemp
    if (Test-Path -LiteralPath $fakeHome) {
        Remove-Item -LiteralPath $fakeHome -Recurse -Force
    }
}

Write-Output 'INSTALL TEST OK'
