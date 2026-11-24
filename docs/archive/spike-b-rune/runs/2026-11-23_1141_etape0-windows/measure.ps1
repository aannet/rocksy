param([string]$Exe, [int]$Runs = 3, [string]$ErrLog = "")
$out = @()
for ($i = 1; $i -le $Runs; $i++) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  if ($ErrLog) { $p = Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe) -PassThru -RedirectStandardError "$ErrLog.$i.txt" }
  else { $p = Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe) -PassThru }
  $win = $null
  while ($sw.ElapsedMilliseconds -lt 15000) {
    $p.Refresh()
    if ($p.HasExited) { break }
    if ($p.MainWindowHandle -ne 0) { $win = $sw.ElapsedMilliseconds; break }
    Start-Sleep -Milliseconds 50
  }
  $rest = 5000 - $sw.ElapsedMilliseconds
  if ($rest -gt 0) { Start-Sleep -Milliseconds $rest }
  $p.Refresh()
  $alive = -not $p.HasExited
  $r = [ordered]@{ run = $i; window_ms = $win; alive_at_5s = $alive }
  if ($alive) {
    $r.title = $p.MainWindowTitle
    $r.responding = $p.Responding
    $r.working_set_mb = [math]::Round($p.WorkingSet64 / 1MB, 1)
    $r.private_mb = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
    $r.cpu_s = [math]::Round($p.TotalProcessorTime.TotalSeconds, 2)
    $r.threads = $p.Threads.Count
    Stop-Process -Id $p.Id -Force
    $p.WaitForExit(3000) | Out-Null
  } else { $r.exit_code = $p.ExitCode }
  $out += [pscustomobject]$r
  Start-Sleep -Milliseconds 1000
}
$out | ConvertTo-Json
