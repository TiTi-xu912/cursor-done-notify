# Cursor stop hook: pop a desktop window when an agent turn finishes.
param(
    [string]$NoticePath,
    [int]$DurationSeconds = 0
)

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

function ConvertTo-SafeId([string]$Id) {
    if ([string]::IsNullOrWhiteSpace($Id)) {
        return 'unknown'
    }
    return ($Id -replace '[^0-9A-Za-z\-_]', '_')
}

function Show-NoticeWindow($Notice) {
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing

    try {
        [System.Windows.Forms.Application]::EnableVisualStyles()
    } catch {
    }

    try {
        Add-Type -TypeDefinition @"
using System.Runtime.InteropServices;
public static class CursorNotifyDpi {
    [DllImport("user32.dll")]
    public static extern bool SetProcessDPIAware();
}
"@ -ErrorAction SilentlyContinue
        [void][CursorNotifyDpi]::SetProcessDPIAware()
    } catch {
    }

    $duration = 12
    if ($Notice.durationSeconds) {
        $duration = [int]$Notice.durationSeconds
    }
    if ($DurationSeconds -gt 0) {
        $duration = $DurationSeconds
    }
    if ($duration -lt 3) { $duration = 3 }
    if ($duration -gt 60) { $duration = 60 }

    $headline = [string]$Notice.headline
    $detail = [string]$Notice.detail
    $snippet = [string]$Notice.snippet
    $accentName = [string]$Notice.accent

    $accent = [System.Drawing.Color]::FromArgb(22, 163, 74)
    $sound = [System.Media.SystemSounds]::Asterisk
    if ($accentName -eq 'orange') {
        $accent = [System.Drawing.Color]::FromArgb(217, 119, 6)
        $sound = [System.Media.SystemSounds]::Exclamation
    } elseif ($accentName -eq 'red') {
        $accent = [System.Drawing.Color]::FromArgb(220, 38, 38)
        $sound = [System.Media.SystemSounds]::Hand
    }

    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'Cursor'
    $form.StartPosition = [System.Windows.Forms.FormStartPosition]::Manual
    $form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedToolWindow
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.TopMost = $true
    $form.ShowInTaskbar = $true
    $form.BackColor = [System.Drawing.Color]::White
    $form.KeyPreview = $true
    try {
        $form.Icon = [System.Drawing.SystemIcons]::Information
    } catch {
    }

    $contentLeft = 22
    $contentWidth = 376
    $y = 16

    $headlineLabel = New-Object System.Windows.Forms.Label
    $headlineLabel.AutoSize = $false
    $headlineLabel.Location = New-Object System.Drawing.Point($contentLeft, $y)
    $headlineLabel.Size = New-Object System.Drawing.Size($contentWidth, 34)
    $headlineLabel.Font = New-Object System.Drawing.Font('Microsoft YaHei UI', 15, [System.Drawing.FontStyle]::Bold)
    $headlineLabel.Text = $headline
    $headlineLabel.ForeColor = [System.Drawing.Color]::FromArgb(17, 24, 39)
    $y += 38

    $detailLabel = New-Object System.Windows.Forms.Label
    $detailLabel.AutoSize = $false
    $detailLabel.Location = New-Object System.Drawing.Point($contentLeft, $y)
    $detailLabel.Size = New-Object System.Drawing.Size($contentWidth, 24)
    $detailLabel.Font = New-Object System.Drawing.Font('Microsoft YaHei UI', 9)
    $detailLabel.Text = $detail
    $detailLabel.ForeColor = [System.Drawing.Color]::FromArgb(75, 85, 99)
    $y += 28

    $snippetLabel = $null
    if (-not [string]::IsNullOrWhiteSpace($snippet)) {
        $snippetLabel = New-Object System.Windows.Forms.Label
        $snippetLabel.AutoSize = $false
        $snippetLabel.Location = New-Object System.Drawing.Point($contentLeft, $y)
        $snippetLabel.Size = New-Object System.Drawing.Size($contentWidth, 66)
        $snippetLabel.Font = New-Object System.Drawing.Font('Microsoft YaHei UI', 9)
        $snippetLabel.Text = $snippet
        $snippetLabel.ForeColor = [System.Drawing.Color]::FromArgb(31, 41, 55)
        $y += 72
    }

    $hintLabel = New-Object System.Windows.Forms.Label
    $hintLabel.AutoSize = $false
    $hintLabel.Location = New-Object System.Drawing.Point($contentLeft, $y)
    $hintLabel.Size = New-Object System.Drawing.Size($contentWidth, 22)
    $hintLabel.Font = New-Object System.Drawing.Font('Microsoft YaHei UI', 8)
    $hintLabel.Text = "点击窗口关闭，或 $duration 秒后自动消失"
    $hintLabel.ForeColor = [System.Drawing.Color]::FromArgb(156, 163, 175)
    $y += 28

    $form.ClientSize = New-Object System.Drawing.Size(420, ($y + 8))

    $bar = New-Object System.Windows.Forms.Panel
    $bar.BackColor = $accent
    $bar.Bounds = New-Object System.Drawing.Rectangle(0, 0, 6, $form.ClientSize.Height)
    $form.Controls.Add($bar)
    $form.Controls.Add($headlineLabel)
    $form.Controls.Add($detailLabel)
    if ($snippetLabel) {
        $form.Controls.Add($snippetLabel)
    }
    $form.Controls.Add($hintLabel)

    $screen = [System.Windows.Forms.Screen]::FromPoint([System.Windows.Forms.Cursor]::Position)
    if (-not $screen) {
        $screen = [System.Windows.Forms.Screen]::PrimaryScreen
    }
    $area = $screen.WorkingArea
    $form.Location = New-Object System.Drawing.Point(
        ($area.Right - $form.Width - 18),
        ($area.Bottom - $form.Height - 18)
    )

    $timer = New-Object System.Windows.Forms.Timer
    $timer.Interval = $duration * 1000
    $timer.Add_Tick({
        $timer.Stop()
        $form.Close()
    }.GetNewClosure())

    $closer = { $form.Close() }.GetNewClosure()
    $form.Add_Click($closer)
    foreach ($control in @($form.Controls)) {
        $control.Add_Click($closer)
    }
    $form.Add_KeyDown({
        if ($_.KeyCode -eq [System.Windows.Forms.Keys]::Escape) {
            $form.Close()
        }
    }.GetNewClosure())
    $form.Add_Shown({
        try {
            $marker = Join-Path (Get-StateDir) 'last-popup-ok.txt'
            Write-Utf8File $marker ((Get-Date).ToString('o'))
        } catch {
        }
        $form.Activate()
        $form.BringToFront()
    }.GetNewClosure())

    $sound.Play()
    $timer.Start()
    [void]$form.ShowDialog()
    $timer.Stop()
    $timer.Dispose()
    $form.Dispose()
}

function Save-LastText($Payload) {
    $dir = Join-Path (Get-StateDir) 'responses'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null

    $id = ''
    if ($Payload.conversation_id) {
        $id = [string]$Payload.conversation_id
    }

    $text = ''
    if ($Payload.text) {
        $text = [string]$Payload.text
    }
    $text = ($text -replace '\s+', ' ').Trim()
    if ($text.Length -gt 400) {
        $text = $text.Substring(0, 400)
    }

    $json = (@{ conversation_id = $id; text = $text } | ConvertTo-Json -Compress)
    $path = Join-Path $dir ((ConvertTo-SafeId $id) + '.json')
    Write-Utf8File $path $json
}

function Take-Snippet($Payload) {
    $dir = Join-Path (Get-StateDir) 'responses'
    if (-not (Test-Path -LiteralPath $dir)) {
        return ''
    }

    $id = ''
    if ($Payload -and $Payload.conversation_id) {
        $id = [string]$Payload.conversation_id
    }

    $path = Join-Path $dir ((ConvertTo-SafeId $id) + '.json')
    if (-not (Test-Path -LiteralPath $path)) {
        $latest = Get-ChildItem -LiteralPath $dir -Filter '*.json' |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if ($latest) {
            $path = $latest.FullName
        } else {
            return ''
        }
    }

    $text = ''
    try {
        $saved = (Read-Utf8File $path) | ConvertFrom-Json
        $text = [string]$saved.text
    } catch {
        $text = ''
    }

    try {
        Remove-Item -LiteralPath $path -Force
    } catch {
    }

    $text = ($text -replace '\s+', ' ').Trim()
    if ($text.Length -gt 140) {
        return ($text.Substring(0, 140) + [string][char]0x2026)
    }
    return $text
}

function Build-Notice($Payload) {
    $status = 'completed'
    if ($Payload -and $Payload.PSObject.Properties.Name -contains 'status' -and $Payload.status) {
        $status = [string]$Payload.status
    }

    $project = '当前工作区'
    if ($Payload -and $Payload.workspace_roots) {
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
        if ($names.Count -gt 0) {
            $project = ($names -join [string][char]0x3001)
        }
    }

    $headline = '回答结束了'
    $accent = 'green'
    switch ($status) {
        'aborted' {
            $headline = '这一轮已中断'
            $accent = 'orange'
        }
        'error' {
            $headline = '这一轮出错了'
            $accent = 'red'
        }
        default {
            $headline = '回答结束了'
            $accent = 'green'
        }
    }

    $duration = 12
    if ($env:CURSOR_NOTIFY_SECONDS -match '^\d+$') {
        $duration = [int]$env:CURSOR_NOTIFY_SECONDS
    }

    $snippet = Take-Snippet $Payload
    $detail = "项目：$project · $(Get-Date -Format 'HH:mm')"
    if ([string]::IsNullOrWhiteSpace($snippet)) {
        $detail = "$detail · 回到终端查看结果"
    }

    return @{
        headline        = $headline
        detail          = $detail
        snippet         = $snippet
        accent          = $accent
        durationSeconds = $duration
    }
}

function Start-NoticeProcess($Notice) {
    $dir = Get-StateDir
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $path = Join-Path $dir ("popup-" + [guid]::NewGuid().ToString('n') + '.json')
    Write-Utf8File $path ($Notice | ConvertTo-Json -Compress)

    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $arg = "-NoProfile -NonInteractive -STA -ExecutionPolicy Bypass -File `"$PSCommandPath`" -NoticePath `"$path`""
    Start-Process -FilePath $ps -WindowStyle Hidden -ArgumentList $arg | Out-Null
}

function Read-StdinText {
    $stream = [Console]::OpenStandardInput()
    $memory = New-Object System.IO.MemoryStream
    $buffer = New-Object byte[] 8192
    while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
        $memory.Write($buffer, 0, $read)
    }
    $bytes = $memory.ToArray()
    if ($bytes.Length -eq 0) {
        return ''
    }

    $candidates = New-Object System.Collections.Generic.List[string]
    if ($bytes.Length -ge 2 -and $bytes[0] -eq 255 -and $bytes[1] -eq 254) {
        $candidates.Add([System.Text.Encoding]::Unicode.GetString($bytes)) | Out-Null
    }
    $candidates.Add($Utf8.GetString($bytes)) | Out-Null
    $candidates.Add([System.Text.Encoding]::Unicode.GetString($bytes)) | Out-Null
    $candidates.Add([System.Text.Encoding]::Default.GetString($bytes)) | Out-Null

    foreach ($candidate in $candidates) {
        if ([string]::IsNullOrWhiteSpace($candidate)) {
            continue
        }
        try {
            $null = $candidate.TrimStart([char]0xFEFF).Trim() | ConvertFrom-Json
            return $candidate
        } catch {
        }
    }

    return $candidates[0]
}

$hookRaw = @($input) -join "`n"
if ([string]::IsNullOrWhiteSpace($NoticePath) -and
    [string]::IsNullOrWhiteSpace($hookRaw) -and
    [Console]::IsInputRedirected) {
    $hookRaw = Read-StdinText
}

if (-not [string]::IsNullOrWhiteSpace($NoticePath)) {
    try {
        $notice = (Read-Utf8File $NoticePath) | ConvertFrom-Json
        try {
            Remove-Item -LiteralPath $NoticePath -Force
        } catch {
        }
        Show-NoticeWindow $notice
        exit 0
    } catch {
        try {
            Write-Utf8File (Join-Path (Get-StateDir) 'last-error.txt') ($_ | Out-String)
        } catch {
        }
        exit 1
    }
}

try {
    $payload = $null
    if (-not [string]::IsNullOrWhiteSpace($hookRaw)) {
        $hookRaw = $hookRaw.TrimStart([char]0xFEFF).Trim()
        if (-not [string]::IsNullOrWhiteSpace($hookRaw)) {
            $payload = $hookRaw | ConvertFrom-Json
        }
    }

    $eventName = ''
    if ($payload -and $payload.hook_event_name) {
        $eventName = [string]$payload.hook_event_name
    }

    $hasStatus = $false
    $hasText = $false
    if ($payload) {
        $names = @($payload.PSObject.Properties.Name)
        $hasStatus = $names -contains 'status'
        $hasText = $names -contains 'text'
    }

    if ($eventName -ieq 'afterAgentResponse' -or ((-not $hasStatus) -and $hasText -and $eventName -ine 'stop')) {
        if ($payload) {
            Save-LastText $payload
        }
    } else {
        Start-NoticeProcess (Build-Notice $payload)
    }
} catch {
    try {
        Write-Utf8File (Join-Path (Get-StateDir) 'last-error.txt') ($_ | Out-String)
    } catch {
    }
} finally {
    [Console]::Out.WriteLine('{}')
}

exit 0
