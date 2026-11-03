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
    if ($ToolName -like 'MCP:*') { return 'search' }
    switch -Regex ($ToolName) {
        '^(Read|TabRead)$' { return 'read' }
        '^(Write|StrReplace|Edit|EditNotebook|Delete)$' { return 'write' }
        '^Shell$' { return 'terminal' }
        '^(Grep|Glob|SemanticSearch|WebSearch|WebFetch)$' { return 'search' }
        default { return 'think' }
    }
}

function Get-CurrentPose {
    if ($script:Phase -eq 'idle') { return 'idle' }
    if ($script:Phase -eq 'done' -or $script:Phase -eq 'settling') {
        if ($script:Accent -eq 'red' -or $script:Accent -eq 'orange') { return 'error' }
        return 'done'
    }
    if ($script:Accent -eq 'red') { return 'error' }
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
    $script:GenerationId = ''
    $script:WorkspaceRoot = ''
    $script:UpdatedAt = Get-Date
}

function Initialize-State {
    $script:DoneSeconds = 12
    if ($env:CURSOR_NOTIFY_SECONDS -match '^\d+$') {
        $script:DoneSeconds = [int]$env:CURSOR_NOTIFY_SECONDS
    }
    if ($script:DoneSeconds -lt 3) { $script:DoneSeconds = 3 }
    if ($script:DoneSeconds -gt 60) { $script:DoneSeconds = 60 }

    $script:SessionFields = @(
        'Phase', 'Headline', 'Activity', 'Pose', 'Detail', 'Prompt', 'Project', 'Snippet', 'Accent',
        'StartedAt', 'StepStartedAt', 'Steps', 'History', 'DoneUntil', 'SettleUntil',
        'SoundPlayed', 'SoundPending', 'Summary', 'StopSeen', 'AnswerSeen', 'OpenTools', 'OpenSubagents',
        'GenerationId', 'UpdatedAt', 'WorkspaceRoot'
    )
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
    return $bag
}

function Import-SessionBag($Bag) {
    foreach ($name in $script:SessionFields) {
        Set-Variable -Name $name -Scope Script -Value $Bag[$name]
    }
    if ($null -eq $script:History) {
        $script:History = New-Object System.Collections.Generic.List[string]
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
    $script:Accent = 'blue'
    $script:Summary = ''
    $script:Headline = '正在处理'
    $script:Activity = ''
    $script:Detail = ''
    $script:Pose = 'think'
}

function Test-WorkOutstanding {
    return ($script:OpenTools -gt 0 -or $script:OpenSubagents -gt 0)
}

function Enter-Running {
    if ($script:StopSeen) {
        return
    }
    if ($script:SoundPlayed) {
        $script:SoundPlayed = $false
        $script:SoundPending = $false
    }
    $script:Phase = 'running'
    $script:SettleUntil = $null
    $script:DoneUntil = $null
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
    if ($Status -eq 'error' -or $Status -eq 'aborted') {
        $script:Pose = 'error'
    } else {
        $script:Pose = 'done'
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
    foreach ($key in @($script:SessionOrder)) {
        Import-SessionBag $script:Sessions[$key]
        $script:SessionKey = $key
        if ($script:Phase -eq 'settling' -and $script:SettleUntil -and (Get-Date) -ge $script:SettleUntil) {
            Soft-Complete
            $script:UpdatedAt = Get-Date
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

function Show-SessionWindow($Bag) {
    if (-not $Bag -or [string]::IsNullOrWhiteSpace([string]$Bag.WorkspaceRoot)) {
        return
    }
    $cursor = $null
    $cmd = Get-Command cursor -ErrorAction SilentlyContinue
    if ($cmd) { $cursor = $cmd.Source }
    if ([string]::IsNullOrWhiteSpace($cursor)) {
        return
    }
    Start-Process -FilePath $cursor -ArgumentList @('-r', '--suppress-popups-on-startup', [string]$Bag.WorkspaceRoot) | Out-Null
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
    if ($bad -gt 0) {
        $accent = 'red'
        $pose = 'error'
    } elseif ($running -gt 0) {
        $accent = 'blue'
        $phase = 'running'
        if ($newestRun -and $newestRun.Pose) { $pose = [string]$newestRun.Pose }
        else { $pose = 'think' }
    } elseif ($aborted -gt 0) {
        $accent = 'orange'
        $pose = 'error'
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
    if ($chosen.Accent -eq 'red') {
        [System.Media.SystemSounds]::Hand.Play()
    } elseif ($chosen.Accent -eq 'orange') {
        [System.Media.SystemSounds]::Exclamation.Play()
    } else {
        [System.Media.SystemSounds]::Asterisk.Play()
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
    $newGeneration = ($gen -and $script:GenerationId -and $gen -ne $script:GenerationId)
    if (-not $script:StartedAt -and $eventName -ne 'beforeSubmitPrompt') {
        Reset-Turn
    }
    if ($eventName -eq 'beforeSubmitPrompt' -or $newGeneration) {
        Reset-Turn
    }
    if ($gen) {
        $script:GenerationId = $gen
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

    switch ($eventName) {
        'beforeSubmitPrompt' {
            $prompt = ''
            if ($Payload -and $Payload.prompt) {
                $prompt = [string]$Payload.prompt
            } else {
                $prompt = Get-JsonField $Raw 'prompt'
            }
            $script:Prompt = Shorten $prompt 60
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
            $script:AnswerSeen = $false
            $script:OpenTools++
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
            if ($script:OpenTools -gt 0) { $script:OpenTools-- }
            $script:StepStartedAt = Get-Date
            if ($toolName) {
                $script:Activity = Get-ToolPhrase $toolName $true
            }
            $detail = Get-ToolDetail $Payload $Raw
            if ($detail) {
                $script:Detail = $detail
            }
            Update-AnswerState
        }
        'postToolUseFailure' {
            if ($script:OpenTools -gt 0) { $script:OpenTools-- }
            Enter-Running
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
            $script:OpenSubagents++
            Enter-Running
            $script:Steps++
            $script:StepStartedAt = Get-Date
            $script:Pose = 'think'
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
            if ($script:OpenSubagents -gt 0) { $script:OpenSubagents-- }
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
    $files = @(Get-ChildItem -LiteralPath $dir -Filter '*.json' -ErrorAction SilentlyContinue | Sort-Object Name)
    foreach ($file in $files) {
        $raw = ''
        try {
            $raw = Read-Utf8File $file.FullName
        } catch {
            continue
        }
        try {
            Remove-Item -LiteralPath $file.FullName -Force -ErrorAction SilentlyContinue
        } catch {
        }
        if ([string]::IsNullOrWhiteSpace($raw)) {
            continue
        }
        $payload = $null
        try {
            $payload = $raw | ConvertFrom-Json
        } catch {
            $payload = $null
        }
        try {
            Apply-Event $payload $raw
        } catch {
            try {
                Write-Utf8File (Join-Path (Get-StateDir) 'last-error.txt') ($_ | Out-String)
            } catch {
            }
        }
    }
}

function Write-LiveView {
    $line = @(
        [string]$script:Phase,
        [string]$script:Steps,
        [string]$script:Headline,
        [string]$script:Activity,
        [string]$script:Detail
    ) -join '|'
    try {
        Write-Utf8File (Join-Path (Get-StateDir) 'live-view.txt') $line
    } catch {
    }
}

function Get-MetaText {
    if ($script:Phase -eq 'done') {
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
    if ($script:Phase -eq 'done' -and $script:DoneUntil) {
        $left = [int][Math]::Ceiling(($script:DoneUntil - (Get-Date)).TotalSeconds)
        if ($left -lt 1) { $left = 1 }
        return ('点一下关掉 · ' + $left + 's')
    }
    if ($script:Phase -eq 'settling') {
        return '回复已经写好了'
    }
    if ($script:Phase -eq 'running') {
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

    Initialize-State
    Apply-Event ([pscustomobject]@{ hook_event_name = 'stop'; generation_id = 'g3'; status = 'completed' }) ''
    Apply-Event ([pscustomobject]@{ hook_event_name = 'afterAgentResponse'; generation_id = 'g3'; text = 'out of order' }) ''
    Assert-Phase 'stop before response' 'done'

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

if ($env:CURSOR_NOTIFY_PREVIEW -eq '1') {
    Import-BuddyAssembly
    $form = New-Object CursorDoneBuddy.BuddyForm
    $form.SpritePath = Join-Path $PSScriptRoot 'sprites\diandian-v2-dot.png'
    $dir = Join-Path (Get-StateDir) 'preview'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $shots = @(
        @{ Name = 'think'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在思考'; Detail = ''; Meta = '已用 00:08 · 第 1 步'; Hint = '点一下可以看步骤'; Pose = 'think'; Start = 0; Sweep = 0 },
        @{ Name = 'read'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在读取'; Detail = 'progress-host.ps1'; Meta = '已用 01:12 · 第 4 步'; Hint = '点一下可以看步骤'; Pose = 'read'; Start = 0; Sweep = 0 },
        @{ Name = 'terminal'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在执行命令'; Detail = 'npm test'; Meta = '已用 00:40 · 第 2 步'; Hint = '点一下可以看步骤'; Pose = 'terminal'; Start = 0; Sweep = 0 },
        @{ Name = 'write'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在修改'; Detail = 'BuddyForm.cs'; Meta = '已用 01:40 · 第 6 步'; Hint = '点一下可以看步骤'; Pose = 'write'; Start = 0; Sweep = 0 },
        @{ Name = 'search'; Phase = 'running'; Accent = 'blue'; Headline = '正在处理'; Activity = '正在搜索'; Detail = 'pose'; Meta = '已用 00:20 · 第 3 步'; Hint = '点一下可以看步骤'; Pose = 'search'; Start = 0; Sweep = 0 },
        @{ Name = 'done'; Phase = 'done'; Accent = 'green'; Headline = '回答结束了'; Activity = 'cursor-done-notify'; Detail = '点一下就能关掉这个小家伙。'; Meta = '共用 02:04 · 11 步'; Hint = '点一下关掉 · 12s'; Pose = 'done'; Start = 0; Sweep = 0 },
        @{ Name = 'error'; Phase = 'done'; Accent = 'red'; Headline = '这一轮出错了'; Activity = '可以回到对话里看'; Detail = '有一步没有成功'; Meta = '共用 00:40 · 3 步'; Hint = '点一下关掉 · 12s'; Pose = 'error'; Start = 0; Sweep = 0 }
    )
    foreach ($shot in $shots) {
        $bmp = $form.RenderPreview($shot.Phase, $shot.Accent, $shot.Headline, $shot.Activity, $shot.Detail, $shot.Meta, $shot.Hint, "已收到问题`n正在读取 a.txt", [single]$shot.Start, [single]$shot.Sweep, [string]$shot.Pose)
        $path = Join-Path $dir ($shot.Name + '.png')
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Output $path
    }
    $form.Dispose()
    exit 0
}

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
Write-Utf8File $pidFile ([string]$PID)

try {
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    try { [System.Windows.Forms.Application]::EnableVisualStyles() } catch {}
    Import-BuddyAssembly

    $script:Buddy = New-Object CursorDoneBuddy.BuddyForm
    $script:Buddy.SpritePath = Join-Path $PSScriptRoot 'sprites\diandian-v2-dot.png'
    $script:Buddy.PositionPath = Join-Path (Get-StateDir) 'buddy-position.txt'
    $script:Buddy.Add_DismissRequested({
        $script:Buddy.Close()
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
            $view = Get-Presentation
            $history = [string]$view.History
            $signature = @(
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
    })

    Update-Buddy
    Place-Buddy
    [System.Windows.Forms.Application]::Run($script:Buddy)
} finally {
    try {
        if (Test-Path -LiteralPath $pidFile) {
            $current = (Read-Utf8File $pidFile).Trim()
            if ($current -eq [string]$PID) {
                Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
            }
        }
    } catch {
    }
    try {
        $mutex.ReleaseMutex()
    } catch {
    }
    $mutex.Dispose()
}

exit 0
