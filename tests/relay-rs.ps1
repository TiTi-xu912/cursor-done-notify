# Runs the relay checks from hooks.ps1 against the Rust hook-relay.exe.
# Pass -Relay to test an already built binary; otherwise it is built with cargo.
param([string]$Relay)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $Relay) {
    Push-Location (Join-Path $root 'proto\buddy-rs')
    try {
        cargo build --release --bin hook-relay
        if ($LASTEXITCODE -ne 0) { throw 'relay build failed' }
    } finally {
        Pop-Location
    }
    $Relay = Join-Path $root 'proto\buddy-rs\target\release\hook-relay.exe'
}
$Relay = (Resolve-Path -LiteralPath $Relay).Path
$Relay = (Resolve-Path -LiteralPath $Relay).Path

$fakeHome = Join-Path $env:TEMP ('cursor-done-notify-relaytest-' + [guid]::NewGuid().ToString('n'))
$binDir = Join-Path $fakeHome 'bin with spaces'
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
# No buddy-host.exe next to this copy, so the relay never starts a host.
$copy = Join-Path $binDir 'hook-relay.exe'
Copy-Item -LiteralPath $Relay -Destination $copy

$savedTemp = $env:TEMP
$savedTmp = $env:TMP
$env:TEMP = $fakeHome
$env:TMP = $fakeHome
try {
    $reply = '{"hook_event_name":"beforeSubmitPrompt","conversation_id":"t","prompt":"hi"}' | & $copy continue
    if ($reply -ne '{"continue":true}') { throw "relay reply: $reply" }
    $reply = '{"hook_event_name":"preToolUse","conversation_id":"t","tool_name":"Read"}' | & $copy allow
    if ($reply -ne '{"permission":"allow"}') { throw "allow reply: $reply" }
    $big = 'x' * 40000
    $reply = ('{"hook_event_name":"stop","conversation_id":"t","status":"completed","text":"' + $big + '"}') | & $copy empty
    if ($reply -ne '{}') { throw "empty reply: $reply" }
    $reply = '' | & $copy empty
    if ($reply -ne '{}') { throw "reply without input: $reply" }
} finally {
    $env:TEMP = $savedTemp
    $env:TMP = $savedTmp
}

$inbox = Join-Path $fakeHome 'cursor-done-notify\inbox'
$files = @(Get-ChildItem -LiteralPath $inbox -Filter '*.json' | Sort-Object Name)
$events = @($files | ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 | ConvertFrom-Json })
if ($events.Count -ne 3) { throw "relay wrote $($events.Count) events" }
if ($events[0].prompt -ne 'hi' -or $events[0].conversation_id -ne 't') { throw 'relay changed the prompt event' }
if ($events[0].host_window -and -not $events[0].host_process) { throw 'host window without its process name' }
if ($events[1].PSObject.Properties.Name -contains 'host_window') { throw 'only the first event of a turn records the host window' }
if ($events[2].status -ne 'completed' -or $events[2].text.Length -ne 180) { throw 'large event was not compacted' }
if ($files[0].Name -notmatch '^\d{20}-[0-9a-f]{32}\.json$') { throw "inbox name format: $($files[0].Name)" }
if (@(Get-ChildItem -LiteralPath $inbox -Filter '*.tmp').Count -ne 0) { throw 'relay left a temp file' }

# Startup cost per hook call, the number that matters since Cursor runs it for every tool.
$env:TEMP = $fakeHome
$env:TMP = $fakeHome
try {
    $runs = 20
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt $runs; $i++) {
        $null = '{"hook_event_name":"postToolUse","conversation_id":"t","tool_name":"Read"}' | & $copy empty
    }
    $watch.Stop()
    Write-Output ('relay: {0:N1} ms per call (including PowerShell piping)' -f ($watch.Elapsed.TotalMilliseconds / $runs))
} finally {
    $env:TEMP = $savedTemp
    $env:TMP = $savedTmp
}

# install.ps1 -Engine rust copies the Rust binaries in and registers the same hooks.
$installRoot = Join-Path $fakeHome 'install root'
$release = Join-Path $installRoot 'proto\buddy-rs\target\release'
New-Item -ItemType Directory -Force -Path $release, (Join-Path $fakeHome '.cursor') | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'install.ps1') -Destination $installRoot
Copy-Item -LiteralPath $Relay -Destination $release
Copy-Item -LiteralPath (Join-Path (Split-Path -Parent $Relay) 'buddy-host.exe') -Destination $release
$savedProfile = $env:USERPROFILE
$savedPath = $env:Path
$env:USERPROFILE = $fakeHome
$env:TEMP = $fakeHome
$env:TMP = $fakeHome
$env:CURSOR_NOTIFY_SKIP_STOP = '1'
# Hide cargo so the installer takes the binaries that are already there.
$env:Path = ($env:Path -split ';' | Where-Object { $_ -and -not (Test-Path -LiteralPath (Join-Path $_ 'cargo.exe')) }) -join ';'
try {
    & (Join-Path $installRoot 'install.ps1') -Engine rust | Out-Null
    $hooks = Get-Content -LiteralPath (Join-Path $fakeHome '.cursor\hooks.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $stop = @($hooks.hooks.stop | ForEach-Object { [string]$_.command })
    # The installer writes the long path; CI runners hand out an 8.3 TEMP like RUNNER~1.
    if (-not ('CursorDoneNotify.LongPath' -as [type])) {
        Add-Type -Namespace CursorDoneNotify -Name LongPath -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint bufferLength);
'@
    }
    $buffer = New-Object System.Text.StringBuilder 1024
    [void][CursorDoneNotify.LongPath]::GetLongPathName($installRoot, $buffer, [uint32]$buffer.Capacity)
    $expected = "& '$(Join-Path $buffer.ToString() 'hook-relay.exe')' empty"
    if (@($stop | Where-Object { $_ -eq $expected }).Count -ne 1) { throw "rust install hook: $($stop -join ' | ')" }
    foreach ($name in @('hook-relay.exe', 'buddy-host.exe')) {
        $want = (Get-FileHash -LiteralPath (Join-Path $release $name)).Hash
        $got = (Get-FileHash -LiteralPath (Join-Path $installRoot $name)).Hash
        if ($want -ne $got) { throw "rust install did not copy $name" }
    }
} finally {
    $env:USERPROFILE = $savedProfile
    $env:TEMP = $savedTemp
    $env:TMP = $savedTmp
    $env:Path = $savedPath
    Remove-Item Env:\CURSOR_NOTIFY_SKIP_STOP -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $fakeHome -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output 'RUST RELAY TEST OK'
