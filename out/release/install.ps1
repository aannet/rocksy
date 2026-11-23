<#
.SYNOPSIS
  Installe Rocksy pour l'utilisateur courant, sans droits administrateur.
.DESCRIPTION
  - Copie rocksy.exe et WebView2Loader.dll dans %LOCALAPPDATA%\Programs\Rocksy.
  - Cree un raccourci dans le menu Demarrer.
  - Declare Rocksy comme application capable d'ouvrir les .md et .markdown (cles HKCU uniquement) :
    il apparait dans "Ouvrir avec". Windows 11 ne laisse pas un script changer l'application par defaut :
    pour le double-clic, faire une fois clic droit sur un .md > Ouvrir avec > Choisir une autre application > Rocksy > Toujours.
  - Avec -AddToPath : ajoute le dossier au PATH utilisateur, pour lancer `rocksy fichier.md` depuis un terminal.
  Desinstallation : .\install.ps1 -Uninstall (les donnees de %APPDATA%\dev.rocksy.app sont conservees).
#>
param([switch]$AddToPath, [switch]$Uninstall)
$ErrorActionPreference = "Stop"
$dest = Join-Path $env:LOCALAPPDATA "Programs\Rocksy"
$exe = Join-Path $dest "rocksy.exe"
$lnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Rocksy.lnk"
$classes = "HKCU:\Software\Classes"
$progId = "Rocksy.Markdown"

if ($Uninstall) {
  Get-Process rocksy -ErrorAction SilentlyContinue | Stop-Process -Force
  Remove-Item -Recurse -Force $dest -ErrorAction SilentlyContinue
  Remove-Item -Force $lnk -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force "$classes\$progId" -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force "$classes\Applications\rocksy.exe" -ErrorAction SilentlyContinue
  foreach ($ext in ".md", ".markdown") { Remove-ItemProperty -Path "$classes\$ext\OpenWithProgids" -Name $progId -ErrorAction SilentlyContinue }
  $p = [Environment]::GetEnvironmentVariable("Path", "User")
  if ($p -and ($p -split ";") -contains $dest) { [Environment]::SetEnvironmentVariable("Path", (($p -split ";") | Where-Object { $_ -ne $dest }) -join ";", "User") }
  "Rocksy desinstalle."
  return
}

$src = Split-Path -Parent $MyInvocation.MyCommand.Path
foreach ($f in "rocksy.exe", "WebView2Loader.dll") { if (-not (Test-Path (Join-Path $src $f))) { throw "Fichier manquant a cote du script : $f" } }
Get-Process rocksy -ErrorAction SilentlyContinue | Stop-Process -Force
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item -Force (Join-Path $src "rocksy.exe"), (Join-Path $src "WebView2Loader.dll") $dest

$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk); $s.TargetPath = $exe; $s.WorkingDirectory = $dest; $s.Description = "Rocksy"; $s.Save()

$cmd = '"' + $exe + '" "%1"'
New-Item -Force "$classes\$progId\shell\open\command" | Out-Null
Set-ItemProperty "$classes\$progId" -Name "(default)" -Value "Document Markdown"
Set-ItemProperty "$classes\$progId\shell\open\command" -Name "(default)" -Value $cmd
New-Item -Force "$classes\$progId\DefaultIcon" | Out-Null
Set-ItemProperty "$classes\$progId\DefaultIcon" -Name "(default)" -Value "$exe,0"
New-Item -Force "$classes\Applications\rocksy.exe\shell\open\command" | Out-Null
Set-ItemProperty "$classes\Applications\rocksy.exe\shell\open\command" -Name "(default)" -Value $cmd
New-Item -Force "$classes\Applications\rocksy.exe\SupportedTypes" | Out-Null
foreach ($ext in ".md", ".markdown") {
  New-ItemProperty -Force -Path "$classes\Applications\rocksy.exe\SupportedTypes" -Name $ext -Value "" | Out-Null
  New-Item -Force "$classes\$ext\OpenWithProgids" | Out-Null
  New-ItemProperty -Force -Path "$classes\$ext\OpenWithProgids" -Name $progId -Value ([byte[]]@()) -PropertyType None | Out-Null
}
if ($AddToPath) {
  $p = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not (($p -split ";") -contains $dest)) { [Environment]::SetEnvironmentVariable("Path", ($p.TrimEnd(";") + ";" + $dest), "User") }
}
"Rocksy installe dans $dest"
"Double-clic sur les .md : clic droit sur un fichier .md > Ouvrir avec > Choisir une autre application > Rocksy > Toujours."
