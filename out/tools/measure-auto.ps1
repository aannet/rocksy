<#
.SYNOPSIS
  Mesure automatique du demarrage de Rocksy, sans chrono manuel.
.DESCRIPTION
  Lance l'exe N fois, releve :
    - windowMs      : delai jusqu'a l'apparition de la fenetre (MainWindowHandle), scrute toutes les 10 ms ;
    - editorReadyMs : delai entre le debut de main() et le jalon "editor-ready" ecrit par l'app dans timings.jsonl
                      (l'editeur accepte la frappe) ;
    - autres jalons ecrits par l'app (tree-ready, tags-ready...).
  Ce sont des mesures instrumentees par l'app : elles ne remplacent pas le chrono manuel T1/T2 de measure-coldstart.ps1
  (qui inclut la perception a l'ecran), mais elles sont reproductibles et sans intervention.
  Les donnees de l'app sont isolees dans -DataDir (variable ROCKSY_DATA_DIR) : le profil reel n'est pas touche.
.EXAMPLE
  .\measure-auto.ps1 -Exe C:\...\rocksy.exe -File "C:\...\note.md" -Case hors-vault -Runs 5
  .\measure-auto.ps1 -Exe C:\...\rocksy.exe -DataDir C:\...\profil-x20 -Case vault-x20 -Runs 5   # session deja enregistree dans ce profil
#>
param(
  [Parameter(Mandatory=$true)][string]$Exe,
  [string]$File = "",
  [string]$DataDir = "$env:TEMP\rocksy-measure-data",
  [string]$Case = "auto",
  [int]$Runs = 5,
  [int]$SettleMs = 4000,
  [string]$RunsRoot = "",
  [switch]$KeepDataDir
)
$ErrorActionPreference = "Stop"
if (-not $KeepDataDir -and $File -ne "" -and (Test-Path $DataDir)) { Remove-Item -Recurse -Force $DataDir }
New-Item -ItemType Directory -Force $DataDir | Out-Null
$env:ROCKSY_DATA_DIR = $DataDir
$timings = Join-Path $DataDir "timings.jsonl"
$name = [IO.Path]::GetFileNameWithoutExtension($Exe)
$rows = @()
for ($i = 1; $i -le $Runs; $i++) {
  if (Test-Path $timings) { Remove-Item $timings }
  $sw = [Diagnostics.Stopwatch]::StartNew()
  if ($File -ne "") { $p = Start-Process $Exe -ArgumentList ('"' + $File + '"') -PassThru } else { $p = Start-Process $Exe -PassThru }
  $windowMs = $null
  while ($sw.ElapsedMilliseconds -lt 15000) {
    $p.Refresh()
    if ($p.HasExited) { break }
    if ($p.MainWindowHandle -ne 0) { $windowMs = $sw.ElapsedMilliseconds; break }
    Start-Sleep -Milliseconds 10
  }
  Start-Sleep -Milliseconds $SettleMs
  $row = [ordered]@{ run = $i; windowMs = $windowMs }
  if (Test-Path $timings) {
    Get-Content $timings | ForEach-Object {
      $j = $_ | ConvertFrom-Json
      if (-not $row.Contains($j.name)) { $row[$j.name] = [int]$j.sinceProcessStartMs }
    }
  }
  $p.Refresh()
  if (-not $p.HasExited) {
    $row["rssMb"] = [int]((Get-Process -Id $p.Id).WorkingSet64 / 1MB)
    $wv = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$name*" -or $_.CommandLine -like "*dev.rocksy.app*" -or $_.CommandLine -like "*$($DataDir.Replace('\','\\'))*" }
    $row["webviewProcs"] = ($wv | Measure-Object).Count
    $row["webviewRssMb"] = [int](($wv | Measure-Object WorkingSetSize -Sum).Sum / 1MB)
    Stop-Process -Id $p.Id -Force
  }
  $rows += [pscustomobject]$row
  Start-Sleep -Milliseconds 800
}
function Median($v) { $s = @($v | Where-Object { $_ -ne $null } | Sort-Object); if ($s.Count -eq 0) { return $null }; return $s[[int][math]::Floor(($s.Count - 1) / 2)] }
$cols = $rows | ForEach-Object { $_.PSObject.Properties.Name } | Select-Object -Unique | Where-Object { $_ -ne "run" }
$med = [ordered]@{}
foreach ($c in $cols) { $med[$c] = Median ($rows | ForEach-Object { $_.$c }) }
$out = [ordered]@{ case = $Case; exe = $Exe; file = $File; runs = $rows; median = $med; date = (Get-Date -Format s); note = "run 1 = premier lancement de la serie (cache le plus froid)" }
$json = $out | ConvertTo-Json -Depth 5
if ($RunsRoot -ne "") {
  $dir = Join-Path $RunsRoot ((Get-Date -Format "yyyy-MM-dd_HHmm") + "_" + $Case)
  New-Item -ItemType Directory -Force $dir | Out-Null
  $json | Set-Content -Encoding UTF8 (Join-Path $dir "auto.json")
}
$json
