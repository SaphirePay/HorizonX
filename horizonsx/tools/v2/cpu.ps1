# CPU of every HorizonsX engine and bot process over SECONDS: cores busy (1.0 = one core flat out) and % of the
# machine's logical processors. Usage: powershell -File tools/v2/cpu.ps1 [SECONDS] [MATCH]
param([int]$Seconds = 10, [string]$Match = '')
$logical = [Environment]::ProcessorCount
function Snap {
    Get-CimInstance Win32_Process | Where-Object {
        ($_.Name -eq 'horizon-client.exe' -or $_.Name -eq 'horizon-bots.exe') -and ($Match -eq '' -or $_.CommandLine -match [regex]::Escape($Match))
    } | ForEach-Object {
        [pscustomobject]@{ Id = $_.ProcessId; Name = $_.Name; Cmd = $_.CommandLine; Cpu = ($_.KernelModeTime + $_.UserModeTime) / 1e7; Mb = [math]::Round($_.WorkingSetSize / 1MB) }
    }
}
$a = Snap; Start-Sleep -Seconds $Seconds; $b = Snap
foreach ($p in $b) {
    $o = $a | Where-Object Id -eq $p.Id
    if (-not $o) { continue }
    $cores = ($p.Cpu - $o.Cpu) / $Seconds
    $dir = if ($p.Cmd -match '\\\.ramp\\([^\\" ]+)') { $Matches[1] } elseif ($p.Cmd -match '--server\s+(\S+)') { "bots -> $($Matches[1])" } else { '' }
    '{0,-18} {1,-24} cores {2,5:N2}  ({3,5:N1}% of {4} threads)  RAM {5} MB' -f $p.Name, $dir, $cores, (100 * $cores / $logical), $logical, $p.Mb
}
