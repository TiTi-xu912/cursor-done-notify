# Build the buddy and register it with the user-level Cursor hook.
# Pass -Uninstall to stop the buddy and remove only its hook entries.
param(
    [switch]$Uninstall,
    [Alias('EnableRealtime')]
    [switch]$EnablePermissionHooks
)

$ErrorActionPreference = 'Stop'

function Get-Csc {
    $candidates = @(
        (Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'),
        (Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe')
    )
    foreach ($path in $candidates) {
        if (Test-Path -LiteralPath $path) {
            return $path
        }
    }
    throw '找不到 .NET Framework 编译器。请安装 .NET Framework 4.x 后再双击 install.cmd。'
}

function Stop-BuddyHost {
    if ($env:CURSOR_NOTIFY_SKIP_STOP -eq '1') {
        return
    }
    $roots = @($PSScriptRoot, $installRoot) | Select-Object -Unique
    $legacyPattern = ($roots | ForEach-Object { [regex]::Escape((Join-Path $_ 'progress-host.ps1')) }) -join '|'
    $hostPaths = @($roots | ForEach-Object { Join-Path $_ 'buddy-host.exe' })
    $procs = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        ($_.Name -eq 'buddy-host.exe' -and $_.ExecutablePath -and
            @($hostPaths | Where-Object { [string]::Equals($_, (Get-LongPath $_.ExecutablePath), [System.StringComparison]::OrdinalIgnoreCase) }).Count -gt 0) -or
        (($_.Name -eq 'powershell.exe' -or $_.Name -eq 'pwsh.exe') -and $_.CommandLine -match $legacyPattern)
    })
    foreach ($proc in $procs) {
        Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
    }
    if ($procs.Count -gt 0) {
        Start-Sleep -Milliseconds 400
    }
}

function Clear-BuddyState {
    $stateDir = Join-Path $env:TEMP 'cursor-done-notify'
    if (-not (Test-Path -LiteralPath $stateDir)) {
        return
    }
    foreach ($name in @('inbox', 'responses', 'live-view.txt', 'last-error.txt', 'last-popup-ok.txt', 'host.pid', 'quota-cache.txt', 'quota-alert.txt')) {
        Remove-Item -LiteralPath (Join-Path $stateDir $name) -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Build-Buddy {
    $csc = Get-Csc
    $root = $PSScriptRoot
    $relayOut = Join-Path $root 'hook-relay.exe'
    $relaySrc = Join-Path $root 'HookRelay.cs'
    $hostOut = Join-Path $root 'buddy-host.exe'
    $hostSrc = @('BuddyForm.cs', 'Quota.cs', 'Host.cs', 'HostApp.cs') | ForEach-Object { Join-Path $root $_ }
    $buildDir = Join-Path $env:TEMP ('cursor-done-notify-build-' + [guid]::NewGuid().ToString('n'))
    $relayTemp = Join-Path $buildDir 'hook-relay.exe'
    $hostTemp = Join-Path $buildDir 'buddy-host.exe'
    New-Item -ItemType Directory -Force -Path $buildDir | Out-Null
    try {
        & $csc /nologo /warn:4 /codepage:65001 /target:exe "/out:$relayTemp" $relaySrc
        if ($LASTEXITCODE -ne 0) { throw 'hook-relay.exe 编译失败' }
        & $csc /nologo /warn:4 /codepage:65001 /target:winexe "/out:$hostTemp" /r:System.Drawing.dll /r:System.Windows.Forms.dll /r:System.Web.Extensions.dll $hostSrc
        if ($LASTEXITCODE -ne 0) { throw 'buddy-host.exe 编译失败' }

        Stop-BuddyHost
        [System.IO.File]::Copy($relayTemp, $relayOut, $true)
        [System.IO.File]::Copy($hostTemp, $hostOut, $true)
        Remove-Item -LiteralPath (Join-Path $root 'BuddyForm.dll') -Force -ErrorAction SilentlyContinue
    } finally {
        Remove-Item -LiteralPath $buildDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Escape-JsonText([string]$Text) {
    if ($null -eq $Text) {
        return ''
    }
    $builder = New-Object System.Text.StringBuilder
    foreach ($ch in $Text.ToCharArray()) {
        $code = [int]$ch
        if ($code -eq 8) { [void]$builder.Append('\b') }
        elseif ($code -eq 9) { [void]$builder.Append('\t') }
        elseif ($code -eq 10) { [void]$builder.Append('\n') }
        elseif ($code -eq 12) { [void]$builder.Append('\f') }
        elseif ($code -eq 13) { [void]$builder.Append('\r') }
        elseif ($code -eq 34) { [void]$builder.Append('\"') }
        elseif ($code -eq 92) { [void]$builder.Append('\\') }
        elseif ($code -lt 32) { [void]$builder.Append(('\u{0:x4}' -f $code)) }
        else { [void]$builder.Append($ch) }
    }
    return $builder.ToString()
}

function ConvertTo-JsonValue($Value) {
    if ($null -eq $Value) { return 'null' }
    if ($Value -is [bool]) {
        if ($Value) { return 'true' }
        return 'false'
    }
    if ($Value -is [byte] -or $Value -is [int16] -or $Value -is [int] -or $Value -is [int64] -or $Value -is [double] -or $Value -is [decimal] -or $Value -is [single]) {
        return ([System.Xml.XmlConvert]::ToString($Value))
    }
    if ($Value -is [string]) {
        return '"' + (Escape-JsonText $Value) + '"'
    }
    if ($Value -is [System.Collections.IDictionary]) {
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($key in @($Value.Keys)) {
            $parts.Add(('{0}: {1}' -f (ConvertTo-JsonValue ([string]$key)), (ConvertTo-JsonValue $Value[$key]))) | Out-Null
        }
        return '{ ' + ($parts -join ', ') + ' }'
    }
    # ArrayList also reports -is [pscustomobject] in Windows PowerShell, so lists must win.
    if ($Value -is [System.Array] -or $Value -is [System.Collections.IList]) {
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($item in @($Value)) {
            $parts.Add((ConvertTo-JsonValue $item)) | Out-Null
        }
        return '[ ' + ($parts -join ', ') + ' ]'
    }
    if ($Value -is [pscustomobject]) {
        $hash = @{}
        foreach ($property in $Value.PSObject.Properties) {
            $hash[$property.Name] = $property.Value
        }
        return ConvertTo-JsonValue $hash
    }
    return (ConvertTo-JsonValue ([string]$Value))
}

function ConvertTo-PowerShellLiteral([string]$Text) {
    return "'" + ($Text -replace "'", "''") + "'"
}

# The same folder can be spelled as an 8.3 short path or a long path; compare the long form.
function Get-LongPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) {
        return ''
    }
    if (-not ('CursorDoneNotify.PathApi' -as [type])) {
        Add-Type -Namespace CursorDoneNotify -Name PathApi -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint bufferLength);
'@
    }
    try {
        $full = [System.IO.Path]::GetFullPath($Path)
    } catch {
        return $Path
    }
    $existing = $full
    $rest = ''
    while ($existing -and -not (Test-Path -LiteralPath $existing)) {
        $leaf = Split-Path -Leaf $existing
        $rest = if ($rest) { Join-Path $leaf $rest } else { $leaf }
        $existing = Split-Path -Parent $existing
    }
    if (-not $existing) {
        return $full
    }
    $buffer = New-Object System.Text.StringBuilder 1024
    $length = [CursorDoneNotify.PathApi]::GetLongPathName($existing, $buffer, [uint32]$buffer.Capacity)
    if ($length -gt 0 -and $length -lt $buffer.Capacity) {
        $existing = $buffer.ToString()
    }
    if ($rest) {
        return (Join-Path $existing $rest)
    }
    return $existing
}

$installRoot = Get-LongPath $PSScriptRoot
$relayExe = Join-Path $installRoot 'hook-relay.exe'

function Get-HookRelayPath([string]$Command) {
    $pattern = "^(?:&\s*'(?<path>(?:[^']|'')+)'|(?<path>.+?\.exe))\s+(?:empty|allow|continue)$"
    $match = [regex]::Match($Command.Trim(), $pattern, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
    if (-not $match.Success) {
        return ''
    }
    return ($match.Groups['path'].Value -replace "''", "'")
}

function Test-OwnedHook([string]$Command) {
    if ([string]::IsNullOrWhiteSpace($Command)) {
        return $false
    }
    $path = Get-HookRelayPath $Command
    if ($path -and [string]::Equals((Get-LongPath $path), $relayExe, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $true
    }
    foreach ($root in @($PSScriptRoot, $installRoot)) {
        foreach ($legacyName in @('notify-done.ps1', 'notify-done.cmd')) {
            $legacyPath = Join-Path $root $legacyName
            if ($Command.IndexOf($legacyPath, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
                return $true
            }
        }
    }
    return $false
}

function Get-KeptHooks($Existing) {
    $kept = New-Object System.Collections.ArrayList
    if ($Existing) {
        foreach ($item in @($Existing)) {
            $cmd = ''
            if ($null -ne $item -and $item.PSObject.Properties.Name -contains 'command') {
                $cmd = [string]$item.command
            }
            if ([string]::IsNullOrWhiteSpace($cmd) -or -not (Test-OwnedHook $cmd)) {
                [void]$kept.Add($item)
            }
        }
    }
    return ,$kept
}

$cursorDir = Join-Path $env:USERPROFILE '.cursor'
$hooksFile = Join-Path $cursorDir 'hooks.json'

function Read-HooksConfig {
    $cfg = @{}
    if (-not (Test-Path -LiteralPath $hooksFile)) {
        $cfg.version = 1
        $cfg.hooks = @{}
        return $cfg
    }
    try {
        $parsed = Get-Content -LiteralPath $hooksFile -Raw -Encoding UTF8 | ConvertFrom-Json
    } catch {
        throw "现有 hooks.json 不是有效 JSON，未作修改：$hooksFile`n$($_.Exception.Message)"
    }
    foreach ($property in $parsed.PSObject.Properties) {
        $cfg[$property.Name] = $property.Value
    }
    if (-not $cfg.ContainsKey('version')) { $cfg.version = 1 }
    $hooks = @{}
    if ($parsed.hooks) {
        foreach ($property in $parsed.hooks.PSObject.Properties) {
            $hooks[$property.Name] = $property.Value
        }
    }
    $cfg.hooks = $hooks
    return $cfg
}

function Write-HooksConfig($Config) {
    $json = ConvertTo-JsonValue $Config
    try {
        $null = $json | ConvertFrom-Json
    } catch {
        throw "生成的 hooks.json 无效，未写入：$($_.Exception.Message)"
    }
    $utf8 = New-Object System.Text.UTF8Encoding $false
    $temp = Join-Path $cursorDir ('hooks.' + [guid]::NewGuid().ToString('n') + '.tmp')
    $backup = "$hooksFile.bak"
    try {
        [System.IO.File]::WriteAllText($temp, $json, $utf8)
        if (Test-Path -LiteralPath $hooksFile) {
            [System.IO.File]::Replace($temp, $hooksFile, $backup, $true)
        } else {
            [System.IO.File]::Move($temp, $hooksFile)
        }
    } finally {
        Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-WithHooksLock([scriptblock]$Action) {
    $mutex = New-Object System.Threading.Mutex($false, 'Local\CursorDoneNotifyHooksConfig')
    $owned = $false
    try {
        try {
            $owned = $mutex.WaitOne([TimeSpan]::FromSeconds(15))
        } catch [System.Threading.AbandonedMutexException] {
            $owned = $true
        }
        if (-not $owned) {
            throw '等待 Cursor hooks 配置锁超时，请稍后重试。'
        }
        & $Action
    } finally {
        if ($owned) {
            try { $mutex.ReleaseMutex() } catch {}
        }
        $mutex.Dispose()
    }
}

if ($Uninstall) {
    Stop-BuddyHost
    if (-not (Test-Path -LiteralPath $hooksFile)) {
        Clear-BuddyState
        Write-Output '没有找到 Cursor hooks。点点已关闭。'
        exit 0
    }
    Invoke-WithHooksLock {
        $cfg = Read-HooksConfig
        $next = @{}
        $removed = 0
        foreach ($name in @($cfg.hooks.Keys)) {
            $before = @($cfg.hooks[$name]).Count
            $kept = Get-KeptHooks $cfg.hooks[$name]
            $removed += ($before - $kept.Count)
            if ($kept.Count -gt 0) {
                $next[$name] = $kept
            }
        }
        if ($removed -gt 0) {
            $cfg.hooks = $next
            Write-HooksConfig $cfg
            Write-Output "已从 hooks 中移除 $removed 条点点记录：$hooksFile"
        } else {
            Write-Output 'hooks 里没有点点的记录。'
        }
    }
    Clear-BuddyState
    Write-Output '点点已关闭。请完全退出 Cursor 后重新打开。'
    exit 0
}

if ($env:CURSOR_NOTIFY_SKIP_BUILD -ne '1') {
    Build-Buddy
}

if (-not (Test-Path -LiteralPath $relayExe)) {
    throw "Missing hook relay: $relayExe"
}
New-Item -ItemType Directory -Force -Path $cursorDir | Out-Null
$command = '& ' + (ConvertTo-PowerShellLiteral $relayExe)

Invoke-WithHooksLock {
    $cfg = Read-HooksConfig
    $hookSpecs = @(
        @{ Name = 'beforeSubmitPrompt'; Reply = 'continue'; Timeout = 5 },
        @{ Name = 'postToolUse'; Reply = 'empty'; Timeout = 5 },
        @{ Name = 'postToolUseFailure'; Reply = 'empty'; Timeout = 5 },
        @{ Name = 'afterAgentThought'; Reply = 'empty'; Timeout = 5 },
        @{ Name = 'afterAgentResponse'; Reply = 'empty'; Timeout = 5 },
        @{ Name = 'subagentStop'; Reply = 'empty'; Timeout = 5 },
        @{ Name = 'stop'; Reply = 'empty'; Timeout = 5 }
    )
    if ($EnablePermissionHooks) {
        $hookSpecs += @(
            @{ Name = 'preToolUse'; Reply = 'allow'; Timeout = 5 },
            @{ Name = 'subagentStart'; Reply = 'allow'; Timeout = 5 }
        )
    }

    foreach ($name in @($cfg.hooks.Keys)) {
        $kept = Get-KeptHooks $cfg.hooks[$name]
        if ($kept.Count -gt 0) {
            $cfg.hooks[$name] = $kept
        } else {
            $cfg.hooks.Remove($name)
        }
    }
    foreach ($spec in $hookSpecs) {
        $kept = Get-KeptHooks $cfg.hooks[$spec.Name]
        [void]$kept.Add(@{
            command = "$command $($spec.Reply)"
            timeout = $spec.Timeout
        })
        $cfg.hooks[$spec.Name] = $kept
    }
    Write-HooksConfig $cfg
}
Write-Output "安装完成：$hooksFile"
if (-not $EnablePermissionHooks) {
    Write-Output '已使用安全模式：不注册会返回 allow 的权限 Hook。若需要实时工具动作，请从 PowerShell 运行 install.ps1 -EnableRealtime。'
}
Write-Output '请完全退出 Cursor 后重新打开。开始一轮 Agent 对话后，点点会出现在桌面右下角。'
