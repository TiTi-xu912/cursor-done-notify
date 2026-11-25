$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $env:TEMP ('cursor-done-notify-quotatest-' + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Force -Path $out | Out-Null

$csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $csc)) {
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'
}
$dll = Join-Path $out 'BuddyForm.dll'
& $csc /nologo /warn:4 /codepage:65001 /target:library "/out:$dll" /r:System.Drawing.dll /r:System.Windows.Forms.dll /r:System.Web.Extensions.dll (Join-Path $root 'BuddyForm.cs') (Join-Path $root 'Quota.cs')
if ($LASTEXITCODE -ne 0) { throw 'quota test compile failed' }

$check = @'
param([string]$Dll)
$ErrorActionPreference = 'Stop'
Add-Type -Path $Dll
$now = Get-Date
$usage = '{"billingCycleStart":"1793770164000","billingCycleEnd":"1796362164000","planUsage":{"totalSpend":18888,"includedSpend":7000,"bonusSpend":11888,"limit":7000,"autoPercentUsed":13.06,"apiPercentUsed":29.2,"totalPercentUsed":14.4},"spendLimitUsage":{"limitType":"user","individualLimit":2000,"individualUsed":550}}'
$plan = '{"planInfo":{"planName":"Pro+","price":"$60/mo"}}'
$s = [CursorDoneBuddy.QuotaParser]::Parse($usage, $plan, 'pro_plus', $now)
if (-not $s.Ok) { throw "parse failed: $($s.Error)" }
if ($s.PlanName -ne 'Pro+') { throw "plan name: $($s.PlanName)" }
if ([Math]::Abs($s.ApiPercent - 29.2) -gt 0.001) { throw "api percent: $($s.ApiPercent)" }
if ($s.TotalSpendCents -ne 18888 -or $s.LimitCents -ne 7000) { throw 'spend fields' }
if ($s.OnDemandLimitCents -ne 2000 -or $s.OnDemandUsedCents -ne 550) { throw 'on-demand fields' }
if ($s.CycleEnd.ToUniversalTime() -ne [DateTime]::new(2026, 12, 4, 5, 29, 24, [DateTimeKind]::Utc)) { throw "cycle end: $($s.CycleEnd.ToUniversalTime().ToString('o'))" }
if ([CursorDoneBuddy.QuotaSnapshot]::Money(18888) -ne '$188.88') { throw 'money format' }
if ([CursorDoneBuddy.QuotaSnapshot]::Percent(0.4) -ne '<1%') { throw 'percent format' }

$round = [CursorDoneBuddy.QuotaSnapshot]::Deserialize($s.Serialize())
if ($round.Serialize() -ne $s.Serialize()) { throw 'cache roundtrip changed the snapshot' }

$fallback = [CursorDoneBuddy.QuotaParser]::Parse('{"planUsage":{"totalSpend":3500,"limit":7000}}', '', 'pro', $now)
if ($fallback.PlanName -ne 'Pro') { throw 'membership fallback' }
if ([Math]::Abs($fallback.HeadlinePercent - 50) -gt 0.001) { throw 'headline percent fallback' }

$bad = [CursorDoneBuddy.QuotaParser]::Parse('[1,2]', '', '', $now)
if ($bad.Ok) { throw 'non-object usage should fail' }

$missing = New-Object CursorDoneBuddy.QuotaService
$missing.DatabasePath = Join-Path $env:TEMP 'no-such-cursor-state.vscdb'
$result = $missing.FetchOnce()
if ($result.Ok -or [string]::IsNullOrEmpty($result.Error)) { throw 'missing database should report an error' }
'QUOTA TEST OK'
'@
$checkPath = Join-Path $out 'check.ps1'
[System.IO.File]::WriteAllText($checkPath, $check, (New-Object System.Text.UTF8Encoding $true))
try {
    $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $checkPath -Dll $dll 2>&1
    $output | ForEach-Object { Write-Output $_ }
    if ($LASTEXITCODE -ne 0 -or ($output -notcontains 'QUOTA TEST OK')) { throw 'quota test failed' }
} finally {
    Remove-Item -LiteralPath $out -Recurse -Force -ErrorAction SilentlyContinue
}
