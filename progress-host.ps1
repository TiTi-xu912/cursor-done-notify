# Floating buddy for Cursor agent progress. Hooks drop JSON in the inbox; this process paints it.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Utf8 = New-Object System.Text.UTF8Encoding $false

function Read-Utf8File([string]$Path) {
    return [System.IO.File]::ReadAllText($Path, $Utf8)
}

function Write-Utf8File([string]$Path, [string]$Text) {
    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Text, $Utf8)
}

function Get-StateDir {
    return (Join-Path $env:TEMP 'cursor-done-notify')
}

function Get-SettingsPath {
    return (Join-Path $env:APPDATA 'cursor-done-notify\settings.txt')
}

function Read-Settings {
    $settings = @{ Mute = $false; Quota = $true; Theme = 'auto'; Quiet = $true }
    $path = Get-SettingsPath
    if (Test-Path -LiteralPath $path) {
        try {
            foreach ($line in ((Read-Utf8File $path) -split "`n")) {
                $pair = $line.Trim() -split '=', 2
                if ($pair.Count -ne 2) { continue }
                switch ($pair[0].Trim()) {
                    'mute' { $settings.Mute = $pair[1].Trim() -eq '1' }
                    'quota' { $settings.Quota = $pair[1].Trim() -ne '0' }
                    'quiet' { $settings.Quiet = $pair[1].Trim() -ne '0' }
                    'theme' {
                        $theme = $pair[1].Trim()
                        if ($theme -eq 'light' -or $theme -eq 'dark') { $settings.Theme = $theme }
                    }
                }
            }
        } catch {
        }
    }
    return $settings
}

function Save-Settings($Settings) {
    $mute = if ($Settings.Mute) { '1' } else { '0' }
    $quota = if ($Settings.Quota) { '1' } else { '0' }
    $quiet = if ($Settings.Quiet) { '1' } else { '0' }
    $text = "mute=$mute`nquota=$quota`nquiet=$quiet`ntheme=$($Settings.Theme)`n"
    try {
        Write-Utf8File (Get-SettingsPath) $text
    } catch {
    }
}

function Format-Money([long]$Cents) {
    return '$' + ([double]$Cents / 100).ToString('0.00', [System.Globalization.CultureInfo]::InvariantCulture)
}

# Spend is sampled from the quota snapshot: the first fetch after a turn starts, and the latest fetch after it ends.
function Update-TurnSpend($Snapshot) {
    if (-not $Snapshot -or -not $Snapshot.Ok -or $Snapshot.TotalSpendCents -lt 0) {
        return
    }
    $spend = [long]$Snapshot.TotalSpendCents
    $fetched = $Snapshot.FetchedAt
    Save-CurrentSession
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        if (-not $bag -or -not $bag.StartedAt) { continue }
        $changed = $false
        if ($bag.SpendStart -lt 0 -and $bag.Phase -ne 'done' -and $fetched -ge $bag.StartedAt.AddSeconds(-30)) {
            $bag.SpendStart = $spend
            $changed = $true
        }
        if ($bag.Phase -eq 'done' -and $bag.DoneAt -and $bag.SpendStart -ge 0 -and $fetched -ge $bag.DoneAt -and $bag.SpendEnd -ne $spend) {
            $bag.SpendEnd = $spend
            $changed = $true
        }
        if ($changed) {
            $script:Sessions[$key] = $bag
            if ($script:SessionKey -eq $key) {
                $script:SpendStart = $bag.SpendStart
                $script:SpendEnd = $bag.SpendEnd
            }
        }
    }
}

function Test-NeedsSpendSample {
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        if (-not $bag) { continue }
        if ($bag.Phase -ne 'done' -and $bag.StartedAt -and $bag.SpendStart -lt 0) { return $true }
    }
    return $false
}

# Cursor books the last requests a few seconds late, so keep sampling briefly after the turn ends.
function Test-NeedsSpendFollowUp($Snapshot) {
    $now = Get-Date
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        if (-not $bag -or $bag.Phase -ne 'done' -or -not $bag.DoneAt -or $bag.SpendStart -lt 0) { continue }
        if ($now -ge $bag.DoneAt.AddSeconds(15)) { continue }
        if (-not $Snapshot -or $Snapshot.FetchedAt -lt $bag.DoneAt.AddSeconds(5)) { return $true }
    }
    return $false
}

function Get-SpendText {
    if ($script:SpendStart -lt 0 -or $script:SpendEnd -lt 0) {
        return ''
    }
    $delta = $script:SpendEnd - $script:SpendStart
    if ($delta -le 0) {
        return ''
    }
    if ($script:SpendShared) {
        return '期间约 ' + (Format-Money $delta)
    }
    return '本轮约 ' + (Format-Money $delta)
}

function Update-QuotaAlert($Snapshot) {
    if (-not $Snapshot -or -not $Snapshot.Ok) {
        return
    }
    $total = [double]$Snapshot.HeadlinePercent
    $api = [double]$Snapshot.ApiPercent
    $name = '本期额度'
    $percent = $total
    if ($api -gt $total) {
        $name = '本期 API 额度'
        $percent = $api
    }
    $level = 0
    if ($percent -ge 100) { $level = 100 }
    elseif ($percent -ge 80) { $level = 80 }
    if ($level -eq 0) {
        return
    }
    $cycle = 'unknown'
    if ($Snapshot.CycleEnd -ne [datetime]::MinValue) {
        $cycle = $Snapshot.CycleEnd.ToUniversalTime().ToString('yyyyMMddHH')
    }
    $alerted = 0
    if (Test-Path -LiteralPath $script:QuotaAlertPath) {
        try {
            $saved = (Read-Utf8File $script:QuotaAlertPath).Trim() -split '\|'
            if ($saved.Count -eq 2 -and $saved[0] -eq $cycle) { $alerted = [int]$saved[1] }
        } catch {
        }
    }
    if ($level -le $alerted) {
        return
    }
    try {
        Write-Utf8File $script:QuotaAlertPath ($cycle + '|' + $level)
    } catch {
    }
    if ($level -ge 100) {
        $script:QuotaNotice = $name + '已用完'
    } else {
        $script:QuotaNotice = $name + '已用 ' + [int][Math]::Floor($percent) + '%'
    }
    $script:QuotaNoticeUntil = (Get-Date).AddSeconds(15)
    if (-not $script:Settings.Mute) {
        [System.Media.SystemSounds]::Exclamation.Play()
    }
}

function Get-QuotaNotice {
    if ($script:QuotaNotice -and $script:QuotaNoticeUntil -and (Get-Date) -lt $script:QuotaNoticeUntil) {
        return $script:QuotaNotice
    }
    return ''
}

function Clear-StaleState {
    $dir = Get-StateDir
    $inbox = Join-Path $dir 'inbox'
    if (Test-Path -LiteralPath $inbox) {
        $cutoff = (Get-Date).AddDays(-1)
        Get-ChildItem -LiteralPath $inbox -File -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -lt $cutoff } |
            Remove-Item -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath (Join-Path $dir 'live-view.txt') -Force -ErrorAction SilentlyContinue
}

function Shorten([string]$Text, [int]$Max) {
    if ([string]::IsNullOrWhiteSpace($Text)) {
        return ''
    }
    $clean = ($Text -replace '\s+', ' ').Trim()
    if ($clean.Length -le $Max) {
        return $clean
    }
    return $clean.Substring(0, $Max) + [string][char]0x2026
}

function Get-JsonField([string]$Text, [string]$Name) {
    if ([string]::IsNullOrWhiteSpace($Text)) {
        return ''
    }
    $pattern = '"' + [regex]::Escape($Name) + '"\s*:\s*"((?:\\.|[^"\\])*)"'
    $match = [regex]::Match($Text, $pattern)
    if (-not $match.Success) {
        return ''
    }
    return [regex]::Unescape($match.Groups[1].Value)
}

function Format-Elapsed($Start) {
    if (-not $Start) {
        return '00:00'
    }
    $span = (Get-Date) - $Start
    if ($span.TotalSeconds -lt 0) {
        $span = [TimeSpan]::Zero
    }
    if ($span.TotalHours -ge 1) {
        return ('{0}:{1:d2}:{2:d2}' -f [int]$span.TotalHours, $span.Minutes, $span.Seconds)
    }
    return ('{0:d2}:{1:d2}' -f [int]$span.TotalMinutes, $span.Seconds)
}

function Get-PropertyText($Object, [string[]]$Names) {
    if ($null -eq $Object) {
        return ''
    }
    if ($Object -is [string]) {
        $trimmed = $Object.Trim()
        if ($trimmed.StartsWith('{') -or $trimmed.StartsWith('[')) {
            try {
                return (Get-PropertyText ($trimmed | ConvertFrom-Json) $Names)
            } catch {
                return (Shorten $Object 80)
            }
        }
        return (Shorten $Object 80)
    }
    foreach ($name in $Names) {
        if ($Object.PSObject.Properties.Name -contains $name -and $Object.$name) {
            return [string]$Object.$name
        }
    }
    return ''
}

function Get-ProjectName($Payload) {
    if (-not $Payload -or -not $Payload.workspace_roots) {
        return ''
    }
    $names = New-Object System.Collections.Generic.List[string]
    foreach ($root in @($Payload.workspace_roots)) {
        $rootText = [string]$root
        if ([string]::IsNullOrWhiteSpace($rootText)) {
            continue
        }
        $name = Split-Path -Leaf $rootText
        if ([string]::IsNullOrWhiteSpace($name)) {
            $name = $rootText
        }
        $names.Add($name) | Out-Null
    }
    if ($names.Count -eq 0) {
        return ''
    }
    return ($names -join [string][char]0x3001)
}

function Get-ToolPhrase([string]$ToolName, [bool]$Finished) {
    $action = '正在处理'
    if ($ToolName -like 'MCP:*') {
        $action = '正在调用外部工具'
    } else {
        switch -Regex ($ToolName) {
            '^(Read|TabRead)$' { $action = '正在读取'; break }
            '^(Write|StrReplace|Edit|EditNotebook|Delete)$' { $action = '正在修改'; break }
            '^Shell$' { $action = '正在执行命令'; break }
            '^(Grep|Glob|SemanticSearch)$' { $action = '正在搜索'; break }
            '^Task$' { $action = '正在派生子任务'; break }
            '^(WebSearch|WebFetch)$' { $action = '正在查找资料'; break }
            default {
                if ([string]::IsNullOrWhiteSpace($ToolName)) {
                    $action = '正在处理'
                } else {
                    $action = "正在使用 $ToolName"
                }
            }
        }
    }
    if ($Finished) {
        return ($action -replace '^正在', '已完成')
    }
    return $action
}

function Get-ToolPose([string]$ToolName) {
    if ($ToolName -like 'MCP:*') { return 'web' }
    switch -Regex ($ToolName) {
        '^(Read|TabRead)$' { return 'read' }
        '^(Write|StrReplace|Edit|EditNotebook|Delete)$' { return 'write' }
        '^Shell$' { return 'terminal' }
        '^(Grep|Glob|SemanticSearch)$' { return 'search' }
        '^(WebSearch|WebFetch)$' { return 'web' }
        '^Task$' { return 'delegate' }
        default { return 'think' }
    }
}

function Get-CurrentPose {
    if ($script:Phase -eq 'idle') { return 'idle' }
    if ($script:Phase -eq 'done' -or $script:Phase -eq 'settling') {
        if ($script:Accent -eq 'red') { return 'error' }
        if ($script:Accent -eq 'orange') {
            if ($script:Pose -eq 'sleep') { return 'sleep' }
            return 'stop'
        }
        return 'done'
    }
    if ($script:Accent -eq 'red') { return 'error' }
    if ($script:Phase -eq 'running' -and -not (Test-WorkOutstanding) -and $script:StepStartedAt -and
        ((Get-Date) - $script:StepStartedAt).TotalSeconds -ge $script:DozeSeconds) {
        return 'sleep'
    }
    if ([string]::IsNullOrWhiteSpace($script:Pose)) { return 'think' }
    return $script:Pose
}

function Get-ToolDetail($Payload, [string]$Raw) {
    $direct = ''
    if ($Payload) {
        $direct = Get-PropertyText $Payload.tool_input @('command', 'file_path', 'path', 'target_file', 'pattern', 'query', 'url', 'glob')
        if ([string]::IsNullOrWhiteSpace($direct) -and $Payload.agent_message) {
            $direct = [string]$Payload.agent_message
        }
        if ([string]::IsNullOrWhiteSpace($direct) -and $Payload.file_path) {
            $direct = [string]$Payload.file_path
        }
        if ([string]::IsNullOrWhiteSpace($direct) -and $Payload.command) {
            $direct = [string]$Payload.command
        }
        if ([string]::IsNullOrWhiteSpace($direct) -and $Payload.task) {
            $direct = [string]$Payload.task
        }
    }
    if ([string]::IsNullOrWhiteSpace($direct) -and $Raw) {
        $direct = Get-JsonField $Raw 'command'
        if ([string]::IsNullOrWhiteSpace($direct)) {
            $direct = Get-JsonField $Raw 'file_path'
        }
    }
    if ($direct -match '^[A-Za-z]:\\' -or $direct -match '^/') {
        $leaf = Split-Path -Leaf $direct
        if (-not [string]::IsNullOrWhiteSpace($leaf)) {
            $direct = $leaf
        }
    }
    return (Shorten $direct 72)
}

function Add-History([string]$Line) {
    $line = Shorten $Line 72
    if ([string]::IsNullOrWhiteSpace($line)) {
        return
    }
    if ($script:History.Count -gt 0 -and $script:History[$script:History.Count - 1] -eq $line) {
        return
    }
    $script:History.Add($line) | Out-Null
    while ($script:History.Count -gt 5) {
        $script:History.RemoveAt(0)
    }
}

function Reset-Fields {
    $script:Phase = 'idle'
    $script:Headline = '点点在这儿'
    $script:Activity = '等你开始提问'
    $script:Pose = 'idle'
    $script:Detail = ''
    $script:Prompt = ''
    $script:Project = ''
    $script:Snippet = ''
    $script:Accent = 'blue'
    $script:StartedAt = $null
    $script:StepStartedAt = $null
    $script:Steps = 0
    $script:History = New-Object System.Collections.Generic.List[string]
    $script:DoneUntil = $null
    $script:SettleUntil = $null
    $script:SoundPlayed = $false
    $script:SoundPending = $false
    $script:Summary = ''
    $script:StopSeen = $false
    $script:AnswerSeen = $false
    $script:OpenTools = 0
    $script:OpenSubagents = 0
    $script:OpenToolIds = @{}
    $script:CompletedToolIds = @{}
    $script:RetiredGenerations = New-Object System.Collections.Generic.List[string]
    $script:GenerationId = ''
    $script:WorkspaceRoot = ''
    $script:SpendStart = -1
    $script:SpendEnd = -1
    $script:SpendShared = $false
    $script:DoneAt = $null
    $script:HostWindow = 0
    $script:HostProcess = ''
    $script:UpdatedAt = Get-Date
}

function Initialize-State {
    $script:DoneSeconds = 12
    if ($env:CURSOR_NOTIFY_SECONDS -match '^\d+$') {
        $script:DoneSeconds = [int]$env:CURSOR_NOTIFY_SECONDS
    }
    if ($script:DoneSeconds -lt 3) { $script:DoneSeconds = 3 }
    if ($script:DoneSeconds -gt 60) { $script:DoneSeconds = 60 }

    $script:StaleMinutes = 180
    if ($env:CURSOR_NOTIFY_STALE_MINUTES -match '^\d+$') {
        $script:StaleMinutes = [int]$env:CURSOR_NOTIFY_STALE_MINUTES
    }
    if ($script:StaleMinutes -lt 10) { $script:StaleMinutes = 10 }
    if ($script:StaleMinutes -gt 1440) { $script:StaleMinutes = 1440 }

    $script:SessionFields = @(
        'Phase', 'Headline', 'Activity', 'Pose', 'Detail', 'Prompt', 'Project', 'Snippet', 'Accent',
        'StartedAt', 'StepStartedAt', 'Steps', 'History', 'DoneUntil', 'SettleUntil',
        'SoundPlayed', 'SoundPending', 'Summary', 'StopSeen', 'AnswerSeen', 'OpenTools', 'OpenSubagents',
        'OpenToolIds', 'CompletedToolIds', 'RetiredGenerations', 'GenerationId', 'UpdatedAt', 'WorkspaceRoot',
        'SpendStart', 'SpendEnd', 'SpendShared', 'DoneAt', 'HostWindow', 'HostProcess'
    )
    $script:DozeSeconds = 90
    $script:QuotaNotice = ''
    $script:QuotaNoticeUntil = $null
    $script:QuotaAlertPath = Join-Path (Get-StateDir) 'quota-alert.txt'
    $script:Settings = Read-Settings
    $script:Sessions = @{}
    $script:SessionOrder = New-Object System.Collections.Generic.List[string]
    $script:SessionKey = ''
    $script:PinnedKey = ''
    $script:LastSignature = ''
    Reset-Fields
}

function Export-SessionBag {
    $bag = @{}
    foreach ($name in $script:SessionFields) {
        $bag[$name] = (Get-Variable -Name $name -Scope Script).Value
    }
    $copy = New-Object System.Collections.Generic.List[string]
    if ($bag.History) {
        foreach ($line in @($bag.History)) {
            $copy.Add([string]$line) | Out-Null
        }
    }
    $bag.History = $copy
    $toolIds = @{}
    if ($bag.OpenToolIds) {
        foreach ($key in @($bag.OpenToolIds.Keys)) {
            $toolIds[$key] = $true
        }
    }
    $bag.OpenToolIds = $toolIds
    $completedToolIds = @{}
    if ($bag.CompletedToolIds) {
        foreach ($key in @($bag.CompletedToolIds.Keys)) {
            $completedToolIds[$key] = $true
        }
    }
    $bag.CompletedToolIds = $completedToolIds
    $retired = New-Object System.Collections.Generic.List[string]
    if ($bag.RetiredGenerations) {
        foreach ($generation in @($bag.RetiredGenerations)) {
            $retired.Add([string]$generation) | Out-Null
        }
    }
    $bag.RetiredGenerations = $retired
    return $bag
}

function Import-SessionBag($Bag) {
    foreach ($name in $script:SessionFields) {
        Set-Variable -Name $name -Scope Script -Value $Bag[$name]
    }
    if ($null -eq $script:History) {
        $script:History = New-Object System.Collections.Generic.List[string]
    }
    if ($null -eq $script:OpenToolIds) {
        $script:OpenToolIds = @{}
    }
    if ($null -eq $script:CompletedToolIds) {
        $script:CompletedToolIds = @{}
    }
    if ($null -eq $script:RetiredGenerations) {
        $script:RetiredGenerations = New-Object System.Collections.Generic.List[string]
    }
}

function Save-CurrentSession {
    if ([string]::IsNullOrWhiteSpace($script:SessionKey)) {
        return
    }
    $script:Sessions[$script:SessionKey] = Export-SessionBag
}

function Use-Session([string]$Key) {
    if ([string]::IsNullOrWhiteSpace($Key)) {
        $Key = 'default'
    }
    if ($script:SessionKey -eq $Key) {
        return
    }
    Save-CurrentSession
    if ($script:Sessions.ContainsKey($Key)) {
        Import-SessionBag $script:Sessions[$Key]
    } else {
        Reset-Fields
        $script:SessionOrder.Add($Key) | Out-Null
    }
    $script:SessionKey = $Key
}

function Get-ConversationId($Payload, [string]$Raw) {
    $id = ''
    if ($Payload -and $Payload.PSObject.Properties.Name -contains 'conversation_id' -and $Payload.conversation_id) {
        $id = [string]$Payload.conversation_id
    }
    if ([string]::IsNullOrWhiteSpace($id) -and $Raw) {
        $id = Get-JsonField $Raw 'conversation_id'
    }
    if ([string]::IsNullOrWhiteSpace($id)) {
        $id = Get-GenerationId $Payload $Raw
    }
    if ([string]::IsNullOrWhiteSpace($id)) {
        return 'default'
    }
    return $id
}

function Reset-Turn {
    $script:Phase = 'running'
    $script:StartedAt = Get-Date
    $script:StepStartedAt = Get-Date
    $script:Steps = 0
    $script:History = New-Object System.Collections.Generic.List[string]
    $script:Snippet = ''
    $script:DoneUntil = $null
    $script:SettleUntil = $null
    $script:SoundPlayed = $false
    $script:SoundPending = $false
    $script:StopSeen = $false
    $script:AnswerSeen = $false
    $script:OpenTools = 0
    $script:OpenSubagents = 0
    $script:OpenToolIds = @{}
    $script:CompletedToolIds = @{}
    $script:Accent = 'blue'
    $script:Summary = ''
    $script:Headline = '正在处理'
    $script:Activity = ''
    $script:Detail = ''
    $script:Pose = 'think'
    $script:SpendStart = -1
    $script:SpendEnd = -1
    $script:SpendShared = Test-OtherSessionRunning
    $script:DoneAt = $null
}

function Test-OtherSessionRunning {
    if (-not $script:Sessions) {
        return $false
    }
    foreach ($key in @($script:Sessions.Keys)) {
        if ($key -eq $script:SessionKey) { continue }
        $phase = $script:Sessions[$key].Phase
        if ($phase -eq 'running' -or $phase -eq 'settling') { return $true }
    }
    return $false
}

function Test-WorkOutstanding {
    return ($script:OpenTools -gt 0 -or $script:OpenSubagents -gt 0)
}

function Enter-Running {
    if ($script:StopSeen) {
        return
    }
    $script:SoundPlayed = $false
    $script:SoundPending = $false
    $script:Phase = 'running'
    $script:SettleUntil = $null
    $script:DoneUntil = $null
    $script:DoneAt = $null
    $script:SpendEnd = -1
    $script:Headline = '正在处理'
    $script:Accent = 'blue'
}

function Update-SummaryLive {
    if ($script:StartedAt) {
        $script:Summary = '已用 ' + (Format-Elapsed $script:StartedAt) + ' · ' + $script:Steps + ' 步'
    }
}

function Hold-Answer {
    if ($script:StopSeen) {
        return
    }
    if (Test-WorkOutstanding) {
        return
    }
    $script:Phase = 'settling'
    $script:DoneUntil = $null
    $script:Headline = '回复写好了'
    $script:Accent = 'green'
    $script:Activity = '可以先看对话'
    $script:Pose = 'done'
    if ($script:Snippet) {
        $script:Detail = $script:Snippet
    }
    $script:SettleUntil = (Get-Date).AddSeconds(8)
    Update-SummaryLive
    Add-History '回复写好了'
}

function Update-AnswerState {
    if ($script:AnswerSeen) {
        Hold-Answer
    }
}

function Complete-Turn([string]$Status) {
    $firstDone = $script:Phase -ne 'done'
    $script:StopSeen = $true
    $script:Phase = 'done'
    $script:SettleUntil = $null
    $script:OpenTools = 0
    $script:OpenSubagents = 0
    $script:OpenToolIds = @{}
    switch ($Status) {
        'aborted' {
            $script:Headline = '这一轮已中断'
            $script:Accent = 'orange'
        }
        'error' {
            $script:Headline = '这一轮出错了'
            $script:Accent = 'red'
        }
        default {
            $script:Headline = '回答结束了'
            $script:Accent = 'green'
            $script:Pose = 'done'
        }
    }
    if ($Status -eq 'error') {
        $script:Pose = 'error'
    } elseif ($Status -eq 'aborted') {
        $script:Pose = 'stop'
    } else {
        $script:Pose = 'done'
    }
    $script:DoneAt = Get-Date
    if (Test-OtherSessionRunning) {
        $script:SpendShared = $true
    }
    $script:Summary = '共用 ' + (Format-Elapsed $script:StartedAt) + ' · ' + $script:Steps + ' 步'
    if ($script:Project) {
        $script:Activity = $script:Project
    } else {
        $script:Activity = '可以回到对话里看'
    }
    if ($script:Snippet) {
        $script:Detail = $script:Snippet
    }
    if ($firstDone) {
        $script:DoneUntil = (Get-Date).AddSeconds($script:DoneSeconds)
        $script:SoundPending = $true
    }
    Add-RetiredGeneration $script:GenerationId
    Add-History $script:Headline
}

function Expire-StaleTurn {
    if ($script:Phase -eq 'done' -or $script:Phase -eq 'idle') {
        return
    }
    $script:StopSeen = $true
    $script:Phase = 'done'
    $script:SettleUntil = $null
    $script:OpenTools = 0
    $script:OpenSubagents = 0
    $script:OpenToolIds = @{}
    $script:Headline = '会话失去连接'
    $script:Activity = '长时间没有收到 Cursor 事件'
    $script:Detail = ''
    $script:Accent = 'orange'
    $script:Pose = 'sleep'
    $script:Summary = '最后活动于 ' + $script:UpdatedAt.ToString('HH:mm')
    $script:DoneUntil = (Get-Date).AddSeconds($script:DoneSeconds)
    $script:SoundPending = $false
    Add-RetiredGeneration $script:GenerationId
    Add-History $script:Headline
}

function Soft-Complete {
    if ($script:StopSeen -or $script:Phase -eq 'done') {
        return
    }
    $script:Phase = 'done'
    $script:SettleUntil = $null
    $script:Headline = '回答结束了'
    $script:Accent = 'green'
    $script:Pose = 'done'
    $script:Summary = '共用 ' + (Format-Elapsed $script:StartedAt) + ' · ' + $script:Steps + ' 步'
    $script:DoneAt = Get-Date
    if (Test-OtherSessionRunning) {
        $script:SpendShared = $true
    }
    if ($script:Project) {
        $script:Activity = $script:Project
    } elseif ([string]::IsNullOrWhiteSpace($script:Activity)) {
        $script:Activity = '可以回到对话里看'
    }
    if ($script:Snippet) {
        $script:Detail = $script:Snippet
    }
    $script:DoneUntil = (Get-Date).AddSeconds($script:DoneSeconds)
    $script:SoundPending = $true
    Add-History $script:Headline
}

function Update-Lifecycle {
    if ($script:SessionOrder.Count -eq 0) {
        if ($script:Phase -eq 'settling' -and $script:SettleUntil -and (Get-Date) -ge $script:SettleUntil) {
            Soft-Complete
        }
        return
    }
    Save-CurrentSession
    $now = Get-Date
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        $settled = $bag.Phase -eq 'settling' -and $bag.SettleUntil -and $now -ge $bag.SettleUntil
        $stale = ($bag.Phase -eq 'running' -or $bag.Phase -eq 'settling') -and
            $bag.UpdatedAt -and $now -ge $bag.UpdatedAt.AddMinutes($script:StaleMinutes)
        if (-not $settled -and -not $stale) {
            continue
        }
        Import-SessionBag $bag
        $script:SessionKey = $key
        if ($settled) {
            Soft-Complete
            $script:UpdatedAt = Get-Date
        }
        if (($script:Phase -eq 'running' -or $script:Phase -eq 'settling') -and
            $script:UpdatedAt -and
            (Get-Date) -ge $script:UpdatedAt.AddMinutes($script:StaleMinutes)) {
            Expire-StaleTurn
        }
        $script:Sessions[$key] = Export-SessionBag
    }
}

function Trim-Sessions {
    while ($script:SessionOrder.Count -gt 8) {
        $removed = $false
        foreach ($key in @($script:SessionOrder)) {
            $bag = $script:Sessions[$key]
            if ($bag.Phase -eq 'done' -or $bag.Phase -eq 'idle') {
                $script:SessionOrder.Remove($key) | Out-Null
                $script:Sessions.Remove($key)
                if ($script:SessionKey -eq $key) { $script:SessionKey = '' }
                $removed = $true
                break
            }
        }
        if (-not $removed) { break }
    }
    while ($script:SessionOrder.Count -gt 16) {
        $oldestKey = $null
        $oldestAt = [datetime]::MaxValue
        foreach ($key in @($script:SessionOrder)) {
            $when = $script:Sessions[$key].UpdatedAt
            if (-not $when) { $when = [datetime]::MinValue }
            if ($when -lt $oldestAt) {
                $oldestAt = $when
                $oldestKey = $key
            }
        }
        if (-not $oldestKey) { break }
        Remove-Session $oldestKey
    }
}

function Get-WorkspaceRoot($Payload) {
    if (-not $Payload -or -not $Payload.workspace_roots) {
        return ''
    }
    foreach ($root in @($Payload.workspace_roots)) {
        $text = [string]$root
        if (-not [string]::IsNullOrWhiteSpace($text)) {
            return $text
        }
    }
    return ''
}

function Get-WorkspaceLeaf($Bag) {
    if (-not $Bag -or [string]::IsNullOrWhiteSpace([string]$Bag.WorkspaceRoot)) {
        return ''
    }
    return (Split-Path -Leaf ([string]$Bag.WorkspaceRoot))
}

function Get-ForegroundWindowInfo {
    if ('CursorDoneBuddy.DesktopWindows' -as [type]) {
        return [CursorDoneBuddy.DesktopWindows]::Foreground()
    }
    return @('', '', '0')
}

# A terminal running the Cursor CLI is identified by its window handle; Cursor itself
# can have several windows for one process, so it is matched by the workspace name.
function Test-TerminalHost($Bag) {
    return ($Bag -and $Bag.HostWindow -and [string]$Bag.HostProcess -ne 'Cursor')
}

function Test-SessionFocused($Bag) {
    $info = @(Get-ForegroundWindowInfo)
    if ($info.Count -lt 3) {
        return $false
    }
    if (Test-TerminalHost $Bag) {
        return ([string]$info[2] -eq [string]$Bag.HostWindow)
    }
    $leaf = Get-WorkspaceLeaf $Bag
    if (-not $leaf) {
        return $false
    }
    return ([string]$info[0] -eq 'Cursor' -and
        ([string]$info[1]).IndexOf($leaf, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
}

function Test-CanJump($Bag) {
    return ((Test-TerminalHost $Bag) -or (Get-WorkspaceLeaf $Bag))
}

function Show-SessionWindow($Bag) {
    $windows = 'CursorDoneBuddy.DesktopWindows' -as [type]
    if ((Test-TerminalHost $Bag) -and $windows -and [CursorDoneBuddy.DesktopWindows]::ActivateHandle([long]$Bag.HostWindow)) {
        return
    }
    $leaf = Get-WorkspaceLeaf $Bag
    if (-not $leaf) {
        return
    }
    if ($windows -and [CursorDoneBuddy.DesktopWindows]::Activate('Cursor', $leaf)) {
        return
    }
    $cmd = Get-Command cursor -ErrorAction SilentlyContinue
    if (-not $cmd) {
        return
    }
    Start-Process -FilePath $cmd.Source -ArgumentList @(('"' + [string]$Bag.WorkspaceRoot + '"')) | Out-Null
}

function Get-SessionLabel($Bag, [string]$Key) {
    if ($Bag.Prompt) { return (Shorten $Bag.Prompt 16) }
    if ($Bag.Project) { return (Shorten $Bag.Project 16) }
    if ($Key.Length -gt 6) { return $Key.Substring(0, 6) }
    return $Key
}

function Get-SessionLine($Bag, [string]$Key) {
    $label = Get-SessionLabel $Bag $Key
    $mark = 'run'
    $state = '进行中'
    if ($Bag.Phase -eq 'done' -and $Bag.Accent -eq 'red') {
        $mark = 'bad'
        $state = '出错'
    } elseif ($Bag.Phase -eq 'done' -and $Bag.Accent -eq 'orange') {
        $mark = 'warn'
        $state = '已中断'
    } elseif ($Bag.Phase -eq 'done') {
        $mark = 'done'
        $state = '已完成'
    } elseif ($Bag.Phase -eq 'settling') {
        $mark = 'wait'
        $state = '收尾'
    } elseif ($Bag.Accent -eq 'orange' -or $Bag.Accent -eq 'red') {
        $mark = 'warn'
        $state = '这一步失败'
    } elseif ($Bag.Activity) {
        $state = [string]$Bag.Activity
    }
    $extra = ''
    if ($Bag.Phase -eq 'running' -and $Bag.Detail) {
        $extra = ' · ' + (Shorten ([string]$Bag.Detail) 18)
    }
    $prefix = ''
    if ($script:PinnedKey -eq $Key) { $prefix = '当前 · ' }
    return ($mark + '|' + $Key + '|' + $prefix + $label + ' · ' + $state + $extra)
}

function Get-SessionRank($Bag) {
    if ($Bag.Phase -eq 'done' -and $Bag.Accent -eq 'red') { return 0 }
    if ($Bag.Phase -eq 'running' -or $Bag.Phase -eq 'settling') { return 1 }
    if ($Bag.Phase -eq 'done' -and $Bag.Accent -eq 'orange') { return 2 }
    return 3
}

function Get-BoardText {
    $rows = New-Object System.Collections.Generic.List[object]
    foreach ($key in @($script:SessionOrder)) {
        if (-not $script:Sessions.ContainsKey($key)) { continue }
        $bag = $script:Sessions[$key]
        $when = $bag.UpdatedAt
        if (-not $when) { $when = [datetime]::MinValue }
        $rows.Add([pscustomobject]@{
            Key = $key
            Bag = $bag
            Rank = (Get-SessionRank $bag)
            Updated = $when
        }) | Out-Null
    }
    $ordered = @($rows | Sort-Object Rank, @{ Expression = 'Updated'; Descending = $true })
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($row in $ordered) {
        $lines.Add((Get-SessionLine $row.Bag $row.Key)) | Out-Null
        if ($lines.Count -ge 8) { break }
    }
    return ($lines -join "`n")
}

function Get-Presentation {
    Save-CurrentSession
    $count = $script:SessionOrder.Count
    if ($count -le 1) {
        if ($count -eq 1) { Use-Session $script:SessionOrder[0] }
        return [pscustomobject]@{
            Phase = [string]$script:Phase
            Accent = [string]$script:Accent
            Headline = [string]$script:Headline
            Activity = [string]$script:Activity
            Detail = [string]$script:Detail
            Meta = (Get-MetaText)
            Hint = (Get-Hint)
            Pose = (Get-CurrentPose)
            History = (Get-HistoryText)
        }
    }

    $running = 0
    $bad = 0
    $aborted = 0
    $finished = 0
    $newestRun = $null
    $newestRunAt = [datetime]::MinValue
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        $when = $bag.UpdatedAt
        if (-not $when) { $when = [datetime]::MinValue }
        if ($bag.Phase -eq 'done' -and $bag.Accent -eq 'red') { $bad++ }
        elseif ($bag.Phase -eq 'done' -and $bag.Accent -eq 'orange') { $aborted++ }
        elseif ($bag.Phase -eq 'done') { $finished++ }
        else {
            $running++
            if ($when -ge $newestRunAt) {
                $newestRunAt = $when
                $newestRun = $bag
            }
        }
    }

    $parts = New-Object System.Collections.Generic.List[string]
    if ($running -gt 0) { $parts.Add(($running.ToString() + ' 个进行中')) | Out-Null }
    if ($bad -gt 0) { $parts.Add(($bad.ToString() + ' 个出错')) | Out-Null }
    if ($aborted -gt 0) { $parts.Add(($aborted.ToString() + ' 个已中断')) | Out-Null }
    if ($finished -gt 0) { $parts.Add(($finished.ToString() + ' 个已完成')) | Out-Null }
    $headline = $parts -join ' · '
    $accent = 'green'
    $pose = 'done'
    $phase = 'done'
    if ($running -gt 0) {
        $phase = 'running'
    }
    if ($bad -gt 0) {
        $accent = 'red'
        $pose = 'error'
    } elseif ($running -gt 0) {
        $accent = 'blue'
        if ($newestRun -and $newestRun.Pose) { $pose = [string]$newestRun.Pose }
        else { $pose = 'think' }
    } elseif ($aborted -gt 0) {
        $accent = 'orange'
        $pose = 'stop'
    }
    $hint = '点一行只处理这一路'
    $activity = ''
    $detail = ''
    $focus = $null
    if ($script:PinnedKey -and $script:Sessions.ContainsKey($script:PinnedKey)) {
        $focus = $script:Sessions[$script:PinnedKey]
        if ($focus.Phase -eq 'done') {
            $script:PinnedKey = ''
            $focus = $null
        }
    }
    if ($focus) {
        $headline = Get-SessionLabel $focus $script:PinnedKey
        $activity = ''
        if ($focus.Activity) { $activity = [string]$focus.Activity }
        $detail = ''
        if ($focus.Detail) { $detail = [string]$focus.Detail }
        $accent = 'blue'
        if ($focus.Accent) { $accent = [string]$focus.Accent }
        $pose = 'think'
        if ($focus.Pose) { $pose = [string]$focus.Pose }
        $phase = 'running'
        $hint = '再点这一行回到总览'
    }
    if (-not $focus -and $phase -eq 'done') {
        $latest = $null
        foreach ($key in @($script:SessionOrder)) {
            $until = $script:Sessions[$key].DoneUntil
            if ($until -and (-not $latest -or $until -gt $latest)) { $latest = $until }
        }
        $left = 1
        if ($latest) {
            $left = [int][Math]::Ceiling(($latest - (Get-Date)).TotalSeconds)
            if ($left -lt 1) { $left = 1 }
        }
        $hint = '点一下关掉 · ' + $left + 's'
    }
    return [pscustomobject]@{
        Phase = $phase
        Accent = $accent
        Headline = $headline
        Activity = [string]$activity
        Detail = [string]$detail
        Meta = ('共 ' + $count + ' 个会话')
        Hint = $hint
        Pose = $pose
        History = (Get-BoardText)
    }
}

function Remove-Session([string]$Key) {
    if ($script:SessionOrder -and $script:SessionOrder.Contains($Key)) {
        $script:SessionOrder.Remove($Key) | Out-Null
    }
    if ($script:Sessions) {
        $script:Sessions.Remove($Key)
    }
    if ($script:PinnedKey -eq $Key) {
        $script:PinnedKey = ''
    }
    if ($script:SessionKey -eq $Key) {
        $script:SessionKey = ''
        if ($script:SessionOrder -and $script:SessionOrder.Count -gt 0) {
            Use-Session $script:SessionOrder[$script:SessionOrder.Count - 1]
        } else {
            Reset-Fields
        }
    }
}

function Invoke-SessionClick([string]$Key) {
    if ([string]::IsNullOrWhiteSpace($Key) -or -not $script:Sessions -or -not $script:Sessions.ContainsKey($Key)) {
        return
    }
    $bag = $script:Sessions[$Key]
    if ($bag.Phase -eq 'done') {
        Remove-Session $Key
        return
    }
    if ($script:PinnedKey -eq $Key) {
        $script:PinnedKey = ''
        return
    }
    $script:PinnedKey = $Key
    Show-SessionWindow $bag
}

function Test-ShouldClose {
    if ($script:SessionOrder.Count -eq 0) {
        return ($script:Phase -eq 'done' -and $script:DoneUntil -and (Get-Date) -ge $script:DoneUntil)
    }
    $latest = $null
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        if ($bag.Phase -ne 'done') { return $false }
        if ($bag.DoneUntil -and (-not $latest -or $bag.DoneUntil -gt $latest)) {
            $latest = $bag.DoneUntil
        }
    }
    if (-not $latest) { return $false }
    return ((Get-Date) -ge $latest)
}

function Play-PendingSounds {
    $chosen = $null
    $chosenKey = ''
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        if (-not $bag.SoundPending) { continue }
        if (-not $chosen -or $bag.Accent -eq 'red' -or ($bag.Accent -eq 'orange' -and $chosen.Accent -ne 'red')) {
            $chosen = $bag
            $chosenKey = $key
        }
    }
    if (-not $chosen) { return }
    $quiet = $script:Settings.Quiet -and (Test-SessionFocused $chosen)
    if ($quiet) {
        $soon = (Get-Date).AddSeconds(4)
        foreach ($key in @($script:SessionOrder)) {
            $bag = $script:Sessions[$key]
            if (-not $bag.SoundPending -or ($bag.DoneUntil -and $bag.DoneUntil -le $soon)) { continue }
            $bag.DoneUntil = $soon
            $script:Sessions[$key] = $bag
            if ($script:SessionKey -eq $key) { $script:DoneUntil = $soon }
        }
    }
    if (-not $script:Settings.Mute -and -not $quiet) {
        if ($chosen.Accent -eq 'red') {
            [System.Media.SystemSounds]::Hand.Play()
        } elseif ($chosen.Accent -eq 'orange') {
            [System.Media.SystemSounds]::Exclamation.Play()
        } else {
            [System.Media.SystemSounds]::Asterisk.Play()
        }
    }
    foreach ($key in @($script:SessionOrder)) {
        $bag = $script:Sessions[$key]
        if (-not $bag.SoundPending) { continue }
        $bag.SoundPending = $false
        $bag.SoundPlayed = $true
        $script:Sessions[$key] = $bag
        if ($script:SessionKey -eq $key) {
            $script:SoundPending = $false
            $script:SoundPlayed = $true
        }
    }
}

function Get-GenerationId($Payload, [string]$Raw) {
    $gen = ''
    if ($Payload -and $Payload.PSObject.Properties.Name -contains 'generation_id' -and $Payload.generation_id) {
        $gen = [string]$Payload.generation_id
    }
    if ([string]::IsNullOrWhiteSpace($gen) -and $Raw) {
        $gen = Get-JsonField $Raw 'generation_id'
    }
    return $gen
}

function Add-RetiredGeneration([string]$Generation) {
    if ([string]::IsNullOrWhiteSpace($Generation)) {
        return
    }
    if (-not $script:RetiredGenerations.Contains($Generation)) {
        $script:RetiredGenerations.Add($Generation) | Out-Null
    }
    while ($script:RetiredGenerations.Count -gt 16) {
        $script:RetiredGenerations.RemoveAt(0)
    }
}

function Test-RetiredGeneration([string]$Generation) {
    return (-not [string]::IsNullOrWhiteSpace($Generation) -and
        $script:RetiredGenerations.Contains($Generation))
}

function Get-ToolUseId($Payload, [string]$Raw) {
    $id = ''
    if ($Payload -and $Payload.PSObject.Properties.Name -contains 'tool_use_id' -and $Payload.tool_use_id) {
        $id = [string]$Payload.tool_use_id
    }
    if ([string]::IsNullOrWhiteSpace($id) -and $Raw) {
        $id = Get-JsonField $Raw 'tool_use_id'
    }
    return $id
}

function Apply-Event($Payload, [string]$Raw) {
    $eventName = ''
    if ($Payload -and $Payload.hook_event_name) {
        $eventName = [string]$Payload.hook_event_name
    } elseif ($Raw) {
        $eventName = Get-JsonField $Raw 'hook_event_name'
    }
    if ([string]::IsNullOrWhiteSpace($eventName)) {
        return
    }

    Use-Session (Get-ConversationId $Payload $Raw)
    try {

    $project = Get-ProjectName $Payload
    if (-not [string]::IsNullOrWhiteSpace($project)) {
        $script:Project = $project
    }
    $root = Get-WorkspaceRoot $Payload
    if (-not [string]::IsNullOrWhiteSpace($root)) {
        $script:WorkspaceRoot = $root
    }

    $gen = Get-GenerationId $Payload $Raw
    if (Test-RetiredGeneration $gen) {
        return
    }
    if ($eventName -eq 'beforeSubmitPrompt') {
        if ($gen -and $script:GenerationId -eq $gen -and $script:StartedAt -and -not $script:StopSeen) {
            return
        }
        if ($gen -and $script:GenerationId -and $gen -ne $script:GenerationId) {
            Add-RetiredGeneration $script:GenerationId
        }
        Reset-Turn
        if ($gen) {
            $script:GenerationId = $gen
        }
    } else {
        if ($gen -and $script:GenerationId -and $gen -ne $script:GenerationId) {
            return
        }
        if (-not $script:StartedAt) {
            Reset-Turn
        }
        if ($gen -and -not $script:GenerationId) {
            $script:GenerationId = $gen
        }
    }

    # stop is the end of this generation. A late response or thought must not restart the ring.
    if ($script:StopSeen) {
        return
    }

    $toolName = ''
    if ($Payload -and $Payload.tool_name) {
        $toolName = [string]$Payload.tool_name
    } elseif ($Raw) {
        $toolName = Get-JsonField $Raw 'tool_name'
    }
    $toolUseId = Get-ToolUseId $Payload $Raw

    switch ($eventName) {
        'beforeSubmitPrompt' {
            $prompt = ''
            if ($Payload -and $Payload.prompt) {
                $prompt = [string]$Payload.prompt
            } else {
                $prompt = Get-JsonField $Raw 'prompt'
            }
            $script:Prompt = Shorten $prompt 60
            $hostWindow = [long]0
            if ($Payload -and $Payload.host_window -and [long]::TryParse([string]$Payload.host_window, [ref]$hostWindow)) {
                $script:HostWindow = $hostWindow
                $script:HostProcess = [string]$Payload.host_process
            }
            $script:Headline = '正在处理'
            $script:Activity = '已收到问题'
            $script:Detail = ''
            if ($script:Prompt) {
                $script:Detail = $script:Prompt
            }
            $script:Accent = 'blue'
            $script:Pose = 'think'
            Add-History $script:Activity
        }
        'preToolUse' {
            if ($toolUseId) {
                if ($script:CompletedToolIds.ContainsKey($toolUseId) -or
                    $script:OpenToolIds.ContainsKey($toolUseId)) {
                    return
                }
                $script:OpenToolIds[$toolUseId] = $true
                $script:OpenTools = $script:OpenToolIds.Count
            } else {
                $script:OpenTools++
            }
            $script:AnswerSeen = $false
            Enter-Running
            $script:Steps++
            $script:StepStartedAt = Get-Date
            $script:Pose = Get-ToolPose $toolName
            $script:Activity = Get-ToolPhrase $toolName $false
            $script:Detail = Get-ToolDetail $Payload $Raw
            $historyLine = $script:Activity
            if ($script:Detail) {
                $historyLine = $historyLine + ' ' + $script:Detail
            }
            Add-History $historyLine
        }
        'postToolUse' {
            $hadOpenTool = $false
            if ($toolUseId) {
                if ($script:CompletedToolIds.ContainsKey($toolUseId)) {
                    return
                }
                $hadOpenTool = $script:OpenToolIds.ContainsKey($toolUseId)
                $script:OpenToolIds.Remove($toolUseId) | Out-Null
                $script:CompletedToolIds[$toolUseId] = $true
                $script:OpenTools = $script:OpenToolIds.Count
            } elseif ($script:OpenTools -gt 0) {
                $hadOpenTool = $true
                $script:OpenTools--
            }
            if (-not $hadOpenTool) {
                Enter-Running
                $script:Steps++
            }
            $script:StepStartedAt = Get-Date
            if ($toolName) {
                $script:Pose = Get-ToolPose $toolName
                $script:Activity = Get-ToolPhrase $toolName $true
            }
            $detail = Get-ToolDetail $Payload $Raw
            if ($detail) {
                $script:Detail = $detail
            }
            if (-not $hadOpenTool) {
                $historyLine = $script:Activity
                if ($script:Detail) {
                    $historyLine = $historyLine + ' ' + $script:Detail
                }
                Add-History $historyLine
            }
            Update-AnswerState
        }
        'postToolUseFailure' {
            $hadOpenTool = $false
            if ($toolUseId) {
                if ($script:CompletedToolIds.ContainsKey($toolUseId)) {
                    return
                }
                $hadOpenTool = $script:OpenToolIds.ContainsKey($toolUseId)
                $script:OpenToolIds.Remove($toolUseId) | Out-Null
                $script:CompletedToolIds[$toolUseId] = $true
                $script:OpenTools = $script:OpenToolIds.Count
            } elseif ($script:OpenTools -gt 0) {
                $hadOpenTool = $true
                $script:OpenTools--
            }
            Enter-Running
            if (-not $hadOpenTool) {
                $script:Steps++
            }
            $script:Accent = 'orange'
            $message = ''
            if ($Payload -and $Payload.error_message) {
                $message = [string]$Payload.error_message
            } else {
                $message = Get-JsonField $Raw 'error_message'
            }
            $script:Pose = 'error'
            $script:Activity = '这一步没有成功'
            $script:Detail = Shorten $message 72
            $script:StepStartedAt = Get-Date
            Add-History ($script:Activity + ' ' + $script:Detail)
        }
        'afterAgentThought' {
            if ($script:AnswerSeen) {
                Update-AnswerState
                return
            }
            Enter-Running
            $script:Steps++
            $script:StepStartedAt = Get-Date
            $script:Pose = 'think'
            $script:Activity = '正在思考'
            $script:Detail = ''
            Add-History '正在思考'
        }
        'afterAgentResponse' {
            $script:AnswerSeen = $true
            $text = ''
            if ($Payload -and $Payload.text) {
                $text = [string]$Payload.text
            } else {
                $text = Get-JsonField $Raw 'text'
            }
            $short = Shorten $text 140
            if ($short) {
                $script:Snippet = $short
            }
            if (Test-WorkOutstanding) {
                $script:Activity = '正在收尾'
                if ($script:Snippet) { $script:Detail = $script:Snippet }
            }
            Update-AnswerState
            Add-History '回复写好了'
        }
        'subagentStart' {
            $script:AnswerSeen = $false
            Enter-Running
            $script:Steps++
            $script:StepStartedAt = Get-Date
            $script:Pose = 'delegate'
            $script:Activity = '正在处理子任务'
            $task = ''
            if ($Payload -and $Payload.task) {
                $task = [string]$Payload.task
            } else {
                $task = Get-JsonField $Raw 'task'
            }
            $script:Detail = Shorten $task 72
            Add-History ($script:Activity + ' ' + $script:Detail)
        }
        'subagentStop' {
            $script:StepStartedAt = Get-Date
            if ($script:AnswerSeen -and -not (Test-WorkOutstanding)) {
                Update-AnswerState
            } else {
                $script:Phase = 'running'
                $script:SettleUntil = $null
                $script:DoneUntil = $null
                $script:Headline = '正在处理'
                $script:Activity = '子任务已返回，继续处理'
                Add-History $script:Activity
            }
        }
        'stop' {
            $status = 'completed'
            if ($Payload -and $Payload.status) {
                $status = [string]$Payload.status
            } elseif ($Raw) {
                $found = Get-JsonField $Raw 'status'
                if ($found) { $status = $found }
            }
            Complete-Turn $status
        }
        default {
            if (-not $script:AnswerSeen) {
                Enter-Running
                $script:Activity = '正在处理'
                $script:StepStartedAt = Get-Date
            }
        }
    }
    } finally {
        $script:UpdatedAt = Get-Date
        Save-CurrentSession
        Trim-Sessions
    }
}

function Read-Inbox {
    $dir = Join-Path (Get-StateDir) 'inbox'
    if (-not (Test-Path -LiteralPath $dir)) {
        return
    }
    $files = [System.IO.Directory]::GetFiles($dir, '*.json')
    if ($files.Length -eq 0) {
        return
    }
    [Array]::Sort($files, [System.StringComparer]::Ordinal)
    foreach ($path in $files) {
        $raw = ''
        try {
            $raw = Read-Utf8File $path
        } catch {
            continue
        }
        if ([string]::IsNullOrWhiteSpace($raw)) {
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
            continue
        }
        $payload = $null
        try {
            $payload = $raw | ConvertFrom-Json
        } catch {
            $payload = $null
        }
        $applied = $false
        try {
            Apply-Event $payload $raw
            $applied = $true
        } catch {
            try {
                Write-Utf8File (Join-Path (Get-StateDir) 'last-error.txt') ($_ | Out-String)
            } catch {
            }
        }
        if ($applied) {
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        } else {
            $failed = [System.IO.Path]::ChangeExtension($path, '.failed')
            Move-Item -LiteralPath $path -Destination $failed -Force -ErrorAction SilentlyContinue
        }
    }
}

function Write-LiveView {
    $line = @(
        [string]$script:Phase,
        [string]$script:Steps,
        [string]$script:Headline,
        [string]$script:Activity,
        ''
    ) -join '|'
    try {
        Write-Utf8File (Join-Path (Get-StateDir) 'live-view.txt') $line
    } catch {
    }
}

function Get-MetaText {
    if ($script:Phase -eq 'done') {
        $spend = Get-SpendText
        if ($spend) {
            return ([string]$script:Summary + ' · ' + $spend)
        }
        return [string]$script:Summary
    }
    if ($script:StartedAt) {
        $text = '已用 ' + (Format-Elapsed $script:StartedAt) + ' · 第 ' + $script:Steps + ' 步'
        if ($script:Project) {
            return ($text + ' · ' + $script:Project)
        }
        return $text
    }
    return '等待这一轮开始'
}

function Get-Hint {
    $jump = Test-CanJump @{ HostWindow = $script:HostWindow; HostProcess = $script:HostProcess; WorkspaceRoot = $script:WorkspaceRoot }
    if ($script:Phase -eq 'done' -and $script:DoneUntil) {
        $left = [int][Math]::Ceiling(($script:DoneUntil - (Get-Date)).TotalSeconds)
        if ($left -lt 1) { $left = 1 }
        if ($jump) { return ('点标题跳回对话 · 点别处关掉 · ' + $left + 's') }
        return ('点一下关掉 · ' + $left + 's')
    }
    if ($script:Phase -eq 'settling') {
        return '回复已经写好了'
    }
    if ($script:Phase -eq 'running') {
        if ($jump) { return '点标题跳回对话 · 点别处看步骤' }
        return '点一下可以看步骤'
    }
    return '等你发来问题'
}

function Get-HistoryText {
    if ($script:History.Count -eq 0) {
        return ''
    }
    return ([string[]]$script:History.ToArray() -join "`n")
}

if ($env:CURSOR_NOTIFY_SELFTEST -eq '1') {
    function Assert-Phase([string]$Name, [string]$Expected) {
        if ($script:Phase -ne $Expected) {
            throw "$Name expected $Expected but was $($script:Phase) (stop=$($script:StopSeen) answer=$($script:AnswerSeen) tools=$($script:OpenTools))"
        }
    }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; generation_id = 'g1'; prompt = '你好'; workspace_roots = @('D:\demo') }) ''
    Assert-Phase 'prompt' 'running'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; generation_id = 'g1'; tool_name = 'Read'; file_path = 'a.txt' }) ''
    Assert-Phase 'tool' 'running'
    if ($script:OpenTools -ne 1) { throw 'open tools' }

    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; generation_id = 'g1'; tool_name = 'Read'; file_path = 'a.txt' }) ''
    Assert-Phase 'tool done' 'running'
    if ($script:OpenTools -ne 0) { throw 'tools not closed' }

    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentResponse'; generation_id = 'g1'; text = '已经好了' }) ''
    Assert-Phase 'answer' 'settling'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentThought'; generation_id = 'g1'; text = 'late' }) ''
    Assert-Phase 'late thought' 'settling'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; generation_id = 'g1'; status = 'completed' }) ''
    Assert-Phase 'stop' 'done'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentResponse'; generation_id = 'g1'; text = 'late answer' }) ''
    Assert-Phase 'late answer' 'done'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; generation_id = 'g1'; tool_name = 'Shell' }) ''
    Assert-Phase 'late tool' 'done'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; generation_id = 'g2'; prompt = '下一问' }) ''
    Assert-Phase 'new turn' 'running'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; generation_id = 'g2'; tool_name = 'Read' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentResponse'; generation_id = 'g2'; text = '先说一句' }) ''
    Assert-Phase 'answer while tool open' 'running'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; generation_id = 'g2'; tool_name = 'Read' }) ''
    Assert-Phase 'tool closed after answer' 'settling'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; generation_id = 'g2'; tool_name = 'Write' }) ''
    Assert-Phase 'work resumed' 'running'

    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; generation_id = 'g2'; tool_name = 'Write' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentResponse'; generation_id = 'g2'; text = '最终回复' }) ''
    Assert-Phase 'final answer' 'settling'
    $script:SettleUntil = (Get-Date).AddSeconds(-1)
    Update-Lifecycle
    Assert-Phase 'soft done' 'done'
    if ($script:StopSeen) { throw 'soft done must not lock the generation' }

    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; generation_id = 'g2'; tool_name = 'Grep' }) ''
    Assert-Phase 'resume after soft done' 'running'
    if ($script:SoundPending) { throw 'resumed work kept a pending completion sound' }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; generation_id = 'tool-ids'; prompt = '工具去重' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; generation_id = 'tool-ids'; tool_name = 'Read'; tool_use_id = 't1' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; generation_id = 'tool-ids'; tool_name = 'Read'; tool_use_id = 't1' }) ''
    if ($script:OpenTools -ne 1 -or $script:Steps -ne 1) { throw 'duplicate tool start was counted twice' }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; generation_id = 'tool-ids'; tool_name = 'Read'; tool_use_id = 't1' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; generation_id = 'tool-ids'; tool_name = 'Read'; tool_use_id = 't1' }) ''
    if ($script:OpenTools -ne 0 -or $script:Steps -ne 1) { throw 'duplicate tool stop changed tool state' }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'late-pre'; generation_id = 'late-pre-g'; prompt = '迟到事件' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; conversation_id = 'late-pre'; generation_id = 'late-pre-g'; tool_name = 'Read'; tool_use_id = 'late-t1' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentResponse'; conversation_id = 'late-pre'; generation_id = 'late-pre-g'; text = '回答已写好' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; conversation_id = 'late-pre'; generation_id = 'late-pre-g'; tool_name = 'Read'; tool_use_id = 'late-t1' }) ''
    if (-not $script:AnswerSeen) { throw 'duplicate tool start cleared a completed answer' }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; conversation_id = 'late-pre'; generation_id = 'late-pre-g'; tool_name = 'Read'; tool_use_id = 'late-t1' }) ''
    Assert-Phase 'duplicate start after answer' 'settling'

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'safe'; generation_id = 'safe-g'; prompt = '安全模式' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUseFailure'; conversation_id = 'safe'; generation_id = 'safe-g'; tool_name = 'Shell'; tool_use_id = 'failed'; error_message = '失败' }) ''
    if ($script:Accent -ne 'orange' -or $script:Steps -ne 1) { throw 'post-only failure was not counted' }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; conversation_id = 'safe'; generation_id = 'safe-g'; tool_name = 'Read'; tool_use_id = 'success'; file_path = 'safe.txt' }) ''
    Assert-Phase 'post-only success' 'running'
    if ($script:Accent -ne 'blue' -or $script:Steps -ne 2 -or $script:Pose -ne 'read') { throw 'post-only success kept stale state' }
    if ((Get-HistoryText) -notmatch '已完成读取') { throw 'post-only success was not added to history' }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; conversation_id = 'safe'; generation_id = 'safe-g'; tool_name = 'Read'; tool_use_id = 'success'; file_path = 'safe.txt' }) ''
    if ($script:Steps -ne 2) { throw 'duplicate post-only success was counted twice' }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'gen-chat'; generation_id = 'old-gen'; prompt = '旧轮次' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; conversation_id = 'gen-chat'; generation_id = 'old-gen'; status = 'completed' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'gen-chat'; generation_id = 'new-gen'; prompt = '新轮次' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; conversation_id = 'gen-chat'; generation_id = 'old-gen'; tool_name = 'Read' }) ''
    Assert-Phase 'old generation ignored' 'running'
    if ($script:GenerationId -ne 'new-gen') { throw 'old generation replaced active generation' }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; generation_id = 'g3'; status = 'completed' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentResponse'; generation_id = 'g3'; text = 'out of order' }) ''
    Assert-Phase 'stop before response' 'done'

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'stale'; generation_id = 'stale-g'; prompt = '失联测试' }) ''
    $script:UpdatedAt = (Get-Date).AddMinutes(-($script:StaleMinutes + 1))
    Save-CurrentSession
    Update-Lifecycle
    Assert-Phase 'stale session' 'done'
    if ($script:Headline -ne '会话失去连接' -or $script:SoundPending) { throw 'stale session did not expire quietly' }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'chat-a'; generation_id = 'ga'; prompt = '修登录' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'preToolUse'; conversation_id = 'chat-a'; generation_id = 'ga'; tool_name = 'Read'; file_path = 'login.cs' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'chat-b'; generation_id = 'gb'; prompt = '看报表' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; conversation_id = 'chat-b'; generation_id = 'gb'; status = 'error' }) ''
    Assert-Phase 'second chat failed' 'done'
    if ($script:Accent -ne 'red') { throw 'second chat should be the error' }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; conversation_id = 'chat-a'; generation_id = 'ga'; tool_name = 'Read'; file_path = 'login.cs' }) ''
    Assert-Phase 'first chat still running' 'running'
    if ($script:Sessions['chat-b'].Phase -ne 'done') { throw 'finished chat was overwritten' }
    if ($script:Sessions['chat-b'].Accent -ne 'red') { throw 'finished chat lost its error' }
    $overview = Get-Presentation
    if ($overview.Phase -ne 'running' -or $overview.Accent -ne 'red') { throw "overview with a running chat must stay running: $($overview.Phase)/$($overview.Accent)" }
    $board = Get-BoardText
    if ($board -notmatch 'bad\|') { throw "board missing error: $board" }
    if ($board -notmatch 'run\|') { throw "board missing running: $board" }
    Invoke-SessionClick 'chat-b'
    if ($script:Sessions.ContainsKey('chat-b')) { throw 'click should drop the finished chat' }
    if ($script:Sessions['chat-a'].Phase -ne 'running') { throw 'click changed the other chat' }
    Invoke-SessionClick 'chat-a'
    if ($script:PinnedKey -ne 'chat-a') { throw 'click should pin the running chat' }
    Invoke-SessionClick 'chat-a'
    if ($script:PinnedKey) { throw 'second click should return to the overview' }

    Initialize-State
    $script:Settings.Mute = $true
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'poses'; generation_id = 'poses-g'; prompt = '动作' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; conversation_id = 'poses'; generation_id = 'poses-g'; tool_name = 'Task'; tool_use_id = 'p1' }) ''
    if ((Get-CurrentPose) -ne 'delegate') { throw "Task pose: $(Get-CurrentPose)" }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'postToolUse'; conversation_id = 'poses'; generation_id = 'poses-g'; tool_name = 'WebFetch'; tool_use_id = 'p2' }) ''
    if ((Get-CurrentPose) -ne 'web') { throw "WebFetch pose: $(Get-CurrentPose)" }
    $script:StepStartedAt = (Get-Date).AddSeconds(-($script:DozeSeconds + 5))
    if ((Get-CurrentPose) -ne 'sleep') { throw "quiet turn should doze: $(Get-CurrentPose)" }
    $script:OpenTools = 1
    if ((Get-CurrentPose) -eq 'sleep') { throw 'open tool must not doze' }
    $script:OpenTools = 0
    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; conversation_id = 'poses'; generation_id = 'poses-g'; status = 'aborted' }) ''
    if ((Get-CurrentPose) -ne 'stop') { throw "aborted pose: $(Get-CurrentPose)" }

    Initialize-State
    $script:Settings.Mute = $true
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'spend'; generation_id = 'spend-g'; prompt = '花费' }) ''
    Update-TurnSpend ([pscustomobject]@{ Ok = $true; TotalSpendCents = 1000; FetchedAt = (Get-Date) })
    if ($script:SpendStart -ne 1000) { throw "spend baseline: $($script:SpendStart)" }
    if (Test-NeedsSpendSample) { throw 'baseline should be satisfied' }
    Update-TurnSpend ([pscustomobject]@{ Ok = $true; TotalSpendCents = 1010; FetchedAt = (Get-Date) })
    if ($script:SpendStart -ne 1000) { throw 'baseline must not move while running' }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; conversation_id = 'spend'; generation_id = 'spend-g'; status = 'completed' }) ''
    Update-TurnSpend ([pscustomobject]@{ Ok = $true; TotalSpendCents = 1042; FetchedAt = (Get-Date).AddSeconds(1) })
    if ((Get-MetaText) -notmatch '本轮约 \$0\.42') { throw "turn spend: $(Get-MetaText)" }
    if (-not (Test-NeedsSpendFollowUp ([pscustomobject]@{ FetchedAt = (Get-Date).AddSeconds(1) }))) { throw 'a fresh finish should keep sampling spend' }
    if (Test-NeedsSpendFollowUp ([pscustomobject]@{ FetchedAt = (Get-Date).AddSeconds(6) })) { throw 'a late enough sample should stop the follow-up' }
    Save-CurrentSession
    $script:Sessions['spend'].DoneAt = (Get-Date).AddSeconds(-20)
    if (Test-NeedsSpendFollowUp $null) { throw 'follow-up must give up after 15 seconds' }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'stale-baseline'; generation_id = 'sb'; prompt = '旧数据' }) ''
    Update-TurnSpend ([pscustomobject]@{ Ok = $true; TotalSpendCents = 500; FetchedAt = (Get-Date).AddMinutes(-10) })
    if ($script:SpendStart -ge 0) { throw 'an old cached snapshot must not become the baseline' }

    Initialize-State
    $script:Settings.Mute = $true
    $script:QuotaAlertPath = Join-Path $env:TEMP ('cursor-done-notify-alert-' + [guid]::NewGuid().ToString('n') + '.txt')
    try {
        $cycleEnd = (Get-Date).AddDays(9)
        Update-QuotaAlert ([pscustomobject]@{ Ok = $true; HeadlinePercent = 60; ApiPercent = 20; CycleEnd = $cycleEnd })
        if (Get-QuotaNotice) { throw 'no alert below 80%' }
        Update-QuotaAlert ([pscustomobject]@{ Ok = $true; HeadlinePercent = 61; ApiPercent = 85.6; CycleEnd = $cycleEnd })
        if ((Get-QuotaNotice) -ne '本期 API 额度已用 85%') { throw "80% alert: $(Get-QuotaNotice)" }
        $script:QuotaNotice = ''
        Update-QuotaAlert ([pscustomobject]@{ Ok = $true; HeadlinePercent = 62; ApiPercent = 88; CycleEnd = $cycleEnd })
        if (Get-QuotaNotice) { throw 'the same level must alert only once per cycle' }
        Update-QuotaAlert ([pscustomobject]@{ Ok = $true; HeadlinePercent = 101; ApiPercent = 90; CycleEnd = $cycleEnd })
        if ((Get-QuotaNotice) -ne '本期额度已用完') { throw "100% alert: $(Get-QuotaNotice)" }
        $script:QuotaNotice = ''
        Update-QuotaAlert ([pscustomobject]@{ Ok = $true; HeadlinePercent = 85; ApiPercent = 10; CycleEnd = $cycleEnd.AddDays(30) })
        if (-not (Get-QuotaNotice)) { throw 'a new billing cycle should alert again' }
    } finally {
        Remove-Item -LiteralPath $script:QuotaAlertPath -Force -ErrorAction SilentlyContinue
    }

    Initialize-State
    $script:Settings.Mute = $false
    $script:Settings.Quiet = $true
    function Get-ForegroundWindowInfo { return @('Cursor', 'login.cs - demo - Cursor', '42') }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'quiet'; generation_id = 'quiet-g'; prompt = '安静'; workspace_roots = @('D:\work\demo') }) ''
    if ((Get-Hint) -notmatch '点标题跳回对话') { throw "hint should offer the jump: $(Get-Hint)" }
    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; conversation_id = 'quiet'; generation_id = 'quiet-g'; status = 'completed' }) ''
    Play-PendingSounds
    if ($script:SoundPending -or -not $script:SoundPlayed) { throw 'quiet finish should still settle the sound flag' }
    if ((($script:DoneUntil - (Get-Date)).TotalSeconds) -gt 5) { throw 'a focused finish should leave quickly' }
    function Get-ForegroundWindowInfo { return @('Cursor', 'other.cs - another-project - Cursor', '43') }
    if (Test-SessionFocused $script:Sessions['quiet']) { throw 'another Cursor window is not this session' }

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'beforeSubmitPrompt'; conversation_id = 'cli'; generation_id = 'cli-g'; prompt = '终端'; host_window = '134608'; host_process = 'WindowsTerminal' }) ''
    Save-CurrentSession
    $bag = $script:Sessions['cli']
    if ($bag.HostWindow -ne 134608 -or $bag.HostProcess -ne 'WindowsTerminal') { throw 'host window was not recorded' }
    if (-not (Test-CanJump $bag)) { throw 'a terminal session without workspace roots can still jump' }
    function Get-ForegroundWindowInfo { return @('WindowsTerminal', 'Some chat title', '134608') }
    if (-not (Test-SessionFocused $bag)) { throw 'the terminal window running the agent counts as focused' }
    function Get-ForegroundWindowInfo { return @('WindowsTerminal', 'Some chat title', '999') }
    if (Test-SessionFocused $bag) { throw 'another terminal window is not this session' }
    function Get-ForegroundWindowInfo { return @('', '', '0') }

    Write-Output 'SELFTEST OK'
    exit 0
}

function Import-BuddyAssembly {
    $dll = Join-Path $PSScriptRoot 'BuddyForm.dll'
    if (-not (Test-Path -LiteralPath $dll)) {
        throw "Missing buddy window: $dll"
    }
    Add-Type -Path $dll
    [CursorDoneBuddy.BuddyForm]::EnableDpi()
}

function Test-QuotaEnabled {
    return ($env:CURSOR_NOTIFY_QUOTA -ne '0')
}

if ($env:CURSOR_NOTIFY_PREVIEW -eq '1') {
    Import-BuddyAssembly
    $form = New-Object CursorDoneBuddy.BuddyForm
    $form.SpritePath = Join-Path $PSScriptRoot 'sprites\diandian.png'
    $epoch = [DateTimeOffset]::Now
    $usage = '{"billingCycleStart":"' + $epoch.AddDays(-20).ToUnixTimeMilliseconds() + '","billingCycleEnd":"' + $epoch.AddDays(10).AddHours(-1).ToUnixTimeMilliseconds() +
        '","planUsage":{"totalSpend":19235,"includedSpend":7000,"bonusSpend":12235,"limit":7000,"autoPercentUsed":13.1,"apiPercentUsed":32.4,"totalPercentUsed":14.7},"spendLimitUsage":{"limitType":"user"}}'
    $plan = '{"planInfo":{"planName":"Pro+"}}'
    $quota = New-Object CursorDoneBuddy.QuotaService
    $quota.Publish([CursorDoneBuddy.QuotaParser]::Parse($usage, $plan, 'pro_plus', (Get-Date)))
    $form.Quota = $quota
    $dir = Join-Path (Get-StateDir) 'preview'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $steps = "已收到问题`n已完成读取 progress-host.ps1`n正在思考`n已完成修改 BuddyForm.cs`n正在执行命令 tests\hooks.ps1"
    $board = "bad|b|看报表 · 出错`nrun|a|修登录 · 正在读取 · login.cs`ndone|c|写说明 · 已完成"
    $shots = @(
        @{ Name = 'think'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在思考'; Detail = ''; Meta = '已用 00:08 · 第 1 步'; Hint = '点标题跳回对话 · 点别处看步骤'; Pose = 'think' },
        @{ Name = 'read'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在读取'; Detail = 'progress-host.ps1'; Meta = '已用 01:12 · 第 4 步 · cursor-done-notify'; Hint = '点标题跳回对话 · 点别处看步骤'; Pose = 'read' },
        @{ Name = 'terminal'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在执行命令'; Detail = 'npm test'; Meta = '已用 00:40 · 第 2 步'; Hint = '点标题跳回对话 · 点别处看步骤'; Pose = 'terminal' },
        @{ Name = 'write'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在修改'; Detail = 'BuddyForm.cs'; Meta = '已用 01:40 · 第 6 步'; Hint = '点标题跳回对话 · 点别处看步骤'; Pose = 'write' },
        @{ Name = 'search'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在搜索'; Detail = 'pose'; Meta = '已用 00:20 · 第 3 步'; Hint = '点标题跳回对话 · 点别处看步骤'; Pose = 'search' },
        @{ Name = 'steps'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在执行命令'; Detail = 'tests\hooks.ps1'; Meta = '已用 02:31 · 第 9 步'; Hint = '点标题跳回对话 · 点别处看步骤'; Pose = 'terminal'; History = $steps; Expanded = $true },
        @{ Name = 'done'; Phase = 'done'; Accent = 'green'; Headline = '回答结束了'; Activity = 'cursor-done-notify'; Detail = '点一下就能关掉这个小家伙。'; Meta = '共用 02:04 · 11 步'; Hint = '点标题跳回对话 · 点别处关掉 · 12s'; Pose = 'done' },
        @{ Name = 'error'; Phase = 'done'; Accent = 'red'; Headline = '这一轮出错了'; Activity = '可以回到对话里看'; Detail = '有一步没有成功'; Meta = '共用 00:40 · 3 步'; Hint = '点标题跳回对话 · 点别处关掉 · 12s'; Pose = 'error' },
        @{ Name = 'board'; Phase = 'running'; Accent = 'red'; Headline = '1 个进行中 · 1 个出错 · 1 个已完成'; Activity = ''; Detail = ''; Meta = '共 3 个会话'; Hint = '点一行只处理这一路'; Pose = 'error'; History = $board },
        @{ Name = 'quota'; Phase = 'done'; Accent = 'green'; Headline = '回答结束了'; Activity = 'cursor-done-notify'; Detail = '额度栏点一下就能展开详情。'; Meta = '共用 03:12 · 14 步 · 本轮约 $0.86'; Hint = '点标题跳回对话 · 点别处关掉 · 12s'; Pose = 'done'; QuotaExpanded = $true },
        @{ Name = 'alert'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在查找资料'; Detail = 'docs.cursor.com'; Meta = '已用 00:52 · 第 5 步'; Hint = '本期 API 额度已用 85%'; HintAlert = $true; Pose = 'web' },
        @{ Name = 'dark'; Phase = 'done'; Accent = 'green'; Headline = '回答结束了'; Activity = 'cursor-done-notify'; Detail = '深色主题会跟着 Windows 设置走。'; Meta = '共用 01:47 · 9 步 · 本轮约 $0.31'; Hint = '点标题跳回对话 · 点别处关掉 · 12s'; Pose = 'done'; QuotaExpanded = $true; Theme = 'dark' }
    )
    foreach ($shot in $shots) {
        $history = ''
        if ($shot.History) { $history = [string]$shot.History }
        $form.Theme = if ($shot.Theme) { [string]$shot.Theme } else { 'light' }
        $form.HintAlert = [bool]$shot.HintAlert
        $form.Expanded = [bool]$shot.Expanded
        $form.QuotaExpanded = [bool]$shot.QuotaExpanded
        $bmp = $form.RenderPreview($shot.Phase, $shot.Accent, $shot.Headline, $shot.Activity, $shot.Detail, $shot.Meta, $shot.Hint, $history, [single]0, [single]0, [string]$shot.Pose)
        $path = Join-Path $dir ($shot.Name + '.png')
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Output $path
    }
    $form.Dispose()
    exit 0
}

Clear-StaleState
Initialize-State

$mutex = New-Object System.Threading.Mutex($false, 'Local\CursorDoneNotifyHost')
$owned = $false
try {
    $owned = $mutex.WaitOne(0)
} catch [System.Threading.AbandonedMutexException] {
    $owned = $true
}
if (-not $owned) {
    exit 0
}

$pidFile = Join-Path (Get-StateDir) 'host.pid'
$processStarted = (Get-Process -Id $PID).StartTime.ToUniversalTime().Ticks
Write-Utf8File $pidFile ("$PID|$processStarted")

try {
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    try { [System.Windows.Forms.Application]::EnableVisualStyles() } catch {}
    Import-BuddyAssembly

    $script:Buddy = New-Object CursorDoneBuddy.BuddyForm
    $script:Buddy.SpritePath = Join-Path $PSScriptRoot 'sprites\diandian.png'
    $script:Buddy.PositionPath = Join-Path (Get-StateDir) 'buddy-position.txt'
    $script:Quota = $null
    $script:QuotaSeenVersion = -1
    $script:LastViewPhase = ''
    if (Test-QuotaEnabled) {
        $script:Quota = New-Object CursorDoneBuddy.QuotaService
        $script:Quota.CachePath = Join-Path (Get-StateDir) 'quota-cache.txt'
        $script:Quota.Start()
    }

    function Apply-Settings {
        $script:Buddy.Theme = [string]$script:Settings.Theme
        if ($script:Quota -and $script:Settings.Quota) {
            $script:Buddy.Quota = $script:Quota
        } else {
            $script:Buddy.Quota = $null
        }
        $script:MuteItem.Checked = [bool]$script:Settings.Mute
        $script:QuietItem.Checked = [bool]$script:Settings.Quiet
        $script:QuotaItem.Checked = [bool]($script:Quota -and $script:Settings.Quota)
        foreach ($item in $script:ThemeItem.DropDownItems) {
            $item.Checked = ([string]$item.Tag -eq [string]$script:Settings.Theme)
        }
        $script:LastSignature = ''
    }

    function New-TrayIcon {
        $path = Join-Path $PSScriptRoot 'sprites\poses\idle-0.png'
        if (-not (Test-Path -LiteralPath $path)) {
            return [System.Drawing.SystemIcons]::Application
        }
        $source = New-Object System.Drawing.Bitmap $path
        try {
            $size = 64
            $side = [int]($source.Width * 0.66)
            $crop = New-Object System.Drawing.Rectangle ([int](($source.Width - $side) / 2)), ([int]($source.Height * 0.02)), $side, $side
            $bitmap = New-Object System.Drawing.Bitmap $size, $size
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.DrawImage($source, (New-Object System.Drawing.Rectangle 0, 0, $size, $size), $crop, [System.Drawing.GraphicsUnit]::Pixel)
            $graphics.Dispose()
            return [System.Drawing.Icon]::FromHandle($bitmap.GetHicon())
        } finally {
            $source.Dispose()
        }
    }

    $script:Menu = New-Object System.Windows.Forms.ContextMenuStrip
    $script:MuteItem = New-Object System.Windows.Forms.ToolStripMenuItem '静音'
    $script:MuteItem.Add_Click({
        $script:Settings.Mute = -not $script:Settings.Mute
        Save-Settings $script:Settings
        Apply-Settings
    })
    $script:QuietItem = New-Object System.Windows.Forms.ToolStripMenuItem '正看着对话时不打扰'
    $script:QuietItem.Add_Click({
        $script:Settings.Quiet = -not $script:Settings.Quiet
        Save-Settings $script:Settings
        Apply-Settings
    })
    $script:QuotaItem = New-Object System.Windows.Forms.ToolStripMenuItem '显示额度栏'
    $script:QuotaItem.Enabled = [bool]$script:Quota
    $script:QuotaItem.Add_Click({
        $script:Settings.Quota = -not $script:Settings.Quota
        Save-Settings $script:Settings
        Apply-Settings
    })
    $script:ThemeItem = New-Object System.Windows.Forms.ToolStripMenuItem '主题'
    foreach ($choice in @(@('auto', '跟随系统'), @('light', '浅色'), @('dark', '深色'))) {
        $themeChoice = New-Object System.Windows.Forms.ToolStripMenuItem $choice[1]
        $themeChoice.Tag = $choice[0]
        $themeChoice.Add_Click({
            param($sender)
            $script:Settings.Theme = [string]$sender.Tag
            Save-Settings $script:Settings
            Apply-Settings
        })
        [void]$script:ThemeItem.DropDownItems.Add($themeChoice)
    }
    $usageItem = New-Object System.Windows.Forms.ToolStripMenuItem '打开 Cursor 用量页'
    $usageItem.Add_Click({
        Start-Process 'https://cursor.com/dashboard?tab=usage' | Out-Null
    })
    $closeItem = New-Object System.Windows.Forms.ToolStripMenuItem '先关掉点点'
    $closeItem.Add_Click({
        $script:Buddy.Close()
    })
    [void]$script:Menu.Items.Add($script:MuteItem)
    [void]$script:Menu.Items.Add($script:QuietItem)
    [void]$script:Menu.Items.Add($script:QuotaItem)
    [void]$script:Menu.Items.Add($script:ThemeItem)
    [void]$script:Menu.Items.Add($usageItem)
    [void]$script:Menu.Items.Add((New-Object System.Windows.Forms.ToolStripSeparator))
    [void]$script:Menu.Items.Add($closeItem)
    $script:Buddy.ContextMenuStrip = $script:Menu

    $script:Tray = New-Object System.Windows.Forms.NotifyIcon
    $script:Tray.Icon = New-TrayIcon
    $script:Tray.Text = '点点'
    $script:Tray.ContextMenuStrip = $script:Menu
    $script:Tray.Visible = $true
    Apply-Settings
    $script:Buddy.Add_DismissRequested({
        $script:Buddy.Close()
    })
    $script:Buddy.Add_FocusRequested({
        Save-CurrentSession
        if ($script:SessionKey -and $script:Sessions.ContainsKey($script:SessionKey)) {
            Show-SessionWindow $script:Sessions[$script:SessionKey]
        }
    })
    $script:Buddy.Add_SessionActivated({
        Invoke-SessionClick ([string]$script:Buddy.ActivatedSessionKey)
        if ($script:SessionOrder.Count -eq 0) {
            $script:Buddy.Close()
            return
        }
        $script:LastSignature = ''
        Update-Buddy
    })

    function Update-Buddy {
        try {
            Read-Inbox
            Update-Lifecycle
            Play-PendingSounds
            if (Test-ShouldClose) {
                $script:Buddy.Close()
                return
            }
            if ($script:Quota) {
                $version = $script:Quota.Version
                if ($version -ne $script:QuotaSeenVersion) {
                    $script:QuotaSeenVersion = $version
                    $snapshot = $script:Quota.Latest
                    Update-TurnSpend $snapshot
                    if ($script:Settings.Quota) {
                        Update-QuotaAlert $snapshot
                    }
                }
                if (Test-NeedsSpendFollowUp $script:Quota.Latest) {
                    $script:Quota.RequestRefresh($true)
                } elseif (Test-NeedsSpendSample) {
                    $script:Quota.RequestRefresh()
                }
            }
            $view = Get-Presentation
            if ($script:Quota -and $view.Phase -eq 'done' -and $script:LastViewPhase -ne 'done') {
                $script:Quota.RequestRefresh()
            }
            $script:LastViewPhase = [string]$view.Phase
            $notice = Get-QuotaNotice
            $hintAlert = -not [string]::IsNullOrEmpty($notice)
            if ($hintAlert) {
                $view.Hint = $notice
            }
            $history = [string]$view.History
            $signature = @(
                [string]$hintAlert,
                [string]$view.Phase,
                [string]$view.Accent,
                [string]$view.Headline,
                [string]$view.Activity,
                [string]$view.Detail,
                [string]$view.Meta,
                [string]$view.Hint,
                [string]$view.Pose,
                $history
            ) -join "`n"
            if ($signature -ne $script:LastSignature) {
                $script:LastSignature = $signature
                $script:Buddy.HintAlert = $hintAlert
                $script:Buddy.CanFocusSession = $script:SessionOrder.Count -le 1 -and [bool](Test-CanJump @{ HostWindow = $script:HostWindow; HostProcess = $script:HostProcess; WorkspaceRoot = $script:WorkspaceRoot })
                $script:Buddy.SetContent(
                    [string]$view.Phase,
                    [string]$view.Accent,
                    [string]$view.Headline,
                    [string]$view.Activity,
                    [string]$view.Detail,
                    [string]$view.Meta,
                    [string]$view.Hint,
                    $history,
                    [string]$view.Pose
                )
            }
            Write-LiveView
        } catch {
            try {
                Write-Utf8File (Join-Path (Get-StateDir) 'last-error.txt') ($_ | Out-String)
            } catch {
            }
        }
    }

    function Place-Buddy {
        $placed = $false
        $posPath = $script:Buddy.PositionPath
        if ($posPath -and (Test-Path -LiteralPath $posPath)) {
            try {
                $parts = @((Read-Utf8File $posPath) -split ',')
                $x = 0
                $y = 0
                if ($parts.Count -ge 2 -and [int]::TryParse($parts[0].Trim(), [ref]$x) -and [int]::TryParse($parts[1].Trim(), [ref]$y)) {
                    $script:Buddy.Location = New-Object System.Drawing.Point($x, $y)
                    $script:Buddy.ClampToScreen()
                    $placed = $true
                }
            } catch {
            }
        }
        if (-not $placed) {
            $screen = [System.Windows.Forms.Screen]::FromPoint([System.Windows.Forms.Cursor]::Position)
            if (-not $screen) {
                $screen = [System.Windows.Forms.Screen]::PrimaryScreen
            }
            $area = $screen.WorkingArea
            $x = $area.Right - $script:Buddy.Width - 20
            $y = $area.Bottom - $script:Buddy.Height - 20
            if ($x -lt $area.Left) { $x = $area.Left }
            if ($y -lt $area.Top) { $y = $area.Top }
            $script:Buddy.Location = New-Object System.Drawing.Point([int]$x, [int]$y)
        }
    }

    $timer = New-Object System.Windows.Forms.Timer
    $timer.Interval = 200
    $timer.Add_Tick({ Update-Buddy })
    $script:Buddy.Add_Shown({
        try {
            Write-Utf8File (Join-Path (Get-StateDir) 'last-popup-ok.txt') ((Get-Date).ToString('o'))
        } catch {
        }
        $timer.Start()
    })
    $script:Buddy.Add_FormClosed({
        $timer.Stop()
        $timer.Dispose()
        if ($script:Quota) {
            $script:Quota.Dispose()
        }
        if ($script:Tray) {
            $script:Tray.Visible = $false
            $script:Tray.Dispose()
            $script:Tray = $null
        }
    })

    Update-Buddy
    Place-Buddy
    [System.Windows.Forms.Application]::Run($script:Buddy)
} finally {
    if ($script:Tray) {
        try {
            $script:Tray.Visible = $false
            $script:Tray.Dispose()
        } catch {
        }
    }
    try {
        if (Test-Path -LiteralPath $pidFile) {
            $current = (Read-Utf8File $pidFile).Trim()
            if (($current -split '\|')[0] -eq [string]$PID) {
                Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
            }
        }
    } catch {
    }
    try {
        Remove-Item -LiteralPath (Join-Path (Get-StateDir) 'live-view.txt') -Force -ErrorAction SilentlyContinue
    } catch {
    }
    try {
        $mutex.ReleaseMutex()
    } catch {
    }
    $mutex.Dispose()
}

exit 0
