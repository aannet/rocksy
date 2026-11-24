<#
.SYNOPSIS
  Mesure le cold start d'Obsidian (protocole 7.2 de l'audit). A lancer dans PowerShell sous Windows.

.DESCRIPTION
  Pour chaque run : ferme Obsidian, attend l'arret complet, lance l'app et chronometre.
    T_window : apparition de la fenetre principale (automatique, poll 10 ms)
    T1       : tu appuies sur F8 des qu'un caractere tape apparait dans la derniere note
    T2       : tu appuies sur F9 des que l'explorateur de fichiers affiche ses dossiers
  F8/F9 sont captes globalement (GetAsyncKeyState), tu peux donc rester dans Obsidian.
  Le bruit humain est de l'ordre de 0,2-0,4 s : la mediane sur plusieurs runs l'absorbe.
  Si tu ne presses pas une touche avant -TimeoutSec, la valeur reste vide.

  Sortie : CSV (cas;run;t_window_s;t1_s;t2_s;total_obsidian_ms;remarque) + medianes.
  Le "Total startup time" d'Obsidian (Parametres > A propos > Start-up time, ou commande
  "Show startup time" selon la version) est demande apres chaque run, Entree pour passer.

.EXAMPLE
  .\measure-coldstart.ps1 -Case Normal -Runs 3
  .\measure-coldstart.ps1 -Case Restreint -Runs 3
  .\measure-coldstart.ps1 -Case Normal -Runs 1 -Label "apres reboot (Drive a froid)"
  # Un spike (resultats ranges dans le dossier du spike) :
  .\measure-coldstart.ps1 -Case Vault-reel -Runs 3 -ObsidianExe <chemin>\rocksy.exe -ProcessName rocksy -RunsRoot ..\runs

  Rangement : chaque session ecrit dans <RunsRoot>\AAAA-MM-JJ_HHMM_<cas>\ (coldstart.csv, meta.json).
#>
param(
  [Parameter(Mandatory)][string]$Case,
  [int]$Runs = 3,
  [string]$Label = "",
  [string]$ObsidianExe = "$env:LOCALAPPDATA\Programs\Obsidian\Obsidian.exe",
  [string]$ProcessName = "Obsidian",   # nom du processus a surveiller (ex. le nom d'un spike)
  [string]$RunsRoot = "$PSScriptRoot\..\runs",   # racine des runs : runs\ a la racine du depot
  [string]$OutCsv = "",                            # vide = dossier date automatique dans $RunsRoot
  [int]$TimeoutSec = 60
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $ObsidianExe)) { throw "Obsidian.exe introuvable : $ObsidianExe (utiliser -ObsidianExe)" }

Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class Keys { [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey); }
'@
$VK_F8 = 0x77; $VK_F9 = 0x78
function Test-Key($vk) { ([Keys]::GetAsyncKeyState($vk) -band 0x8000) -ne 0 }

function Stop-Obsidian {
  Get-Process $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force
  $deadline = (Get-Date).AddSeconds(15)
  while (Get-Process $ProcessName -ErrorAction SilentlyContinue) {
    if ((Get-Date) -gt $deadline) { throw "Obsidian ne s'arrete pas" }
    Start-Sleep -Milliseconds 200
  }
  Start-Sleep -Seconds 2   # laisse retomber les I/O (Drive, cache)
}

function Median($xs) {
  $v = @($xs | Where-Object { $null -ne $_ } | Sort-Object)
  if ($v.Count -eq 0) { return $null }
  $m = [int][math]::Floor($v.Count / 2)
  if ($v.Count % 2) { $v[$m] } else { ($v[$m - 1] + $v[$m]) / 2 }
}

# Un dossier par session de mesure : <RunsRoot>\AAAA-MM-JJ_HHMM_<cas>\ (coldstart.csv + meta.json)
if (-not $OutCsv) {
  $slug = ($Case -replace '[^A-Za-z0-9_-]+', '-').Trim('-')
  $RunDir = Join-Path $RunsRoot ("{0}_{1}" -f (Get-Date -Format 'yyyy-MM-dd_HHmm'), $slug)
  New-Item -ItemType Directory -Force $RunDir | Out-Null
  $OutCsv = Join-Path $RunDir 'coldstart.csv'
} else {
  $RunDir = Split-Path -Parent $OutCsv; if (-not $RunDir) { $RunDir = "." }
}
[pscustomobject]@{
  date = (Get-Date -Format 'o'); cas = $Case; runs = $Runs; label = $Label
  exe = $ObsidianExe; processName = $ProcessName
  machine = $env:COMPUTERNAME; os = [Environment]::OSVersion.VersionString
} | ConvertTo-Json | Out-File (Join-Path $RunDir 'meta.json') -Encoding utf8

if (-not (Test-Path $OutCsv)) {
  "cas;run;t_window_s;t1_s;t2_s;total_obsidian_ms;remarque" | Out-File $OutCsv -Encoding utf8
}

Write-Host "Cas '$Case' : $Runs run(s). F8 = frappe acceptee (T1), F9 = arbre visible (T2)." -ForegroundColor Cyan
if ($Case -match 'restreint') { Write-Host "Rappel : active le mode restreint AVANT, desactive-le APRES (peut etre synchronise par Drive)." -ForegroundColor Yellow }

$rows = @()
for ($i = 1; $i -le $Runs; $i++) {
  Read-Host "Run $i/$Runs - Entree pour fermer Obsidian et lancer le chrono" | Out-Null
  Stop-Obsidian
  # Evite qu'un F8/F9 encore enfonce soit lu comme une mesure
  while ((Test-Key $VK_F8) -or (Test-Key $VK_F9)) { Start-Sleep -Milliseconds 50 }

  $sw = [Diagnostics.Stopwatch]::StartNew()
  Start-Process $ObsidianExe | Out-Null
  $tWin = $null; $t1 = $null; $t2 = $null

  while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec -and ($null -eq $t1 -or $null -eq $t2)) {
    if ($null -eq $tWin) {
      $p = Get-Process $ProcessName -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 }
      if ($p) { $tWin = [math]::Round($sw.Elapsed.TotalSeconds, 2) }
    }
    if ($null -eq $t1 -and (Test-Key $VK_F8)) { $t1 = [math]::Round($sw.Elapsed.TotalSeconds, 2) }
    if ($null -eq $t2 -and (Test-Key $VK_F9)) { $t2 = [math]::Round($sw.Elapsed.TotalSeconds, 2) }
    Start-Sleep -Milliseconds 10
  }

  $total = Read-Host "Total startup time d'Obsidian en ms (Entree pour passer)"
  $note  = Read-Host "Remarque (Entree pour passer)"
  if ($Label) { $note = ($Label + ' ' + $note).Trim() }

  $row = [pscustomobject]@{ Cas=$Case; Run=$i; TWindow=$tWin; T1=$t1; T2=$t2; Total=$total; Note=$note }
  $rows += $row
  ('{0};{1};{2};{3};{4};{5};{6}' -f $Case, $i, $tWin, $t1, $t2, $total, $note) -replace ',', '.' |
    Out-File $OutCsv -Append -Encoding utf8
  Write-Host ("  -> fenetre {0}s | T1 {1}s | T2 {2}s" -f $tWin, $t1, $t2)
}

Write-Host "`nMedianes ($Case, $Runs run(s)) :" -ForegroundColor Green
Write-Host ("  T_window {0}s | T1 {1}s (cible <= 2) | T2 {2}s (cible <= 3)" -f `
  (Median $rows.TWindow), (Median $rows.T1), (Median $rows.T2))
Write-Host "Resultats ecrits dans $RunDir"
