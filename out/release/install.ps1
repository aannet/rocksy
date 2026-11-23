<#
.SYNOPSIS
  Installe Rocksy pour l'utilisateur courant, sans droits administrateur.
.DESCRIPTION
  - Copie rocksy.exe et WebView2Loader.dll dans %LOCALAPPDATA%\Programs\Rocksy.
  - Cree un raccourci dans le menu Demarrer.
  - Declare Rocksy comme application capable d'ouvrir les .md et .markdown (cles HKCU uniquement) :
    il apparait dans "Ouvrir avec", a cote des applications deja declarees (leurs entrees ne sont pas touchees).
    Windows 11 ne laisse pas un script changer l'application par defaut : pour le double-clic, faire une fois
    clic droit sur un .md > Ouvrir avec > Choisir une autre application > Rocksy > Toujours.
  - Avec -AddToPath : ajoute le dossier au PATH utilisateur, pour lancer `rocksy fichier.md` depuis un terminal.
  Mise a jour : relancer le script. Si le Rocksy installe est ouvert, le script lui demande de se fermer
  (comme un clic sur la croix) et attend -CloseTimeoutSec secondes (10 par defaut). S'il reste ouvert, le
  script s'arrete sans rien modifier : fermer Rocksy puis relancer. -ForceClose arrete le processus de force
  (les modifications non enregistrees sont perdues).
  Desinstallation : .\install.ps1 -Uninstall. Retire les fichiers, le raccourci, les cles de registre de
  Rocksy et son entree du PATH. Les donnees de %APPDATA%\dev.rocksy.app sont conservees.
.NOTES
  Fichier en ASCII pur (Windows PowerShell 5.1 lit un script sans BOM en ANSI) : pas d'accents ici.
#>
param(
  [switch]$AddToPath,
  [switch]$Uninstall,
  [switch]$ForceClose,
  [ValidateRange(1, 600)][int]$CloseTimeoutSec = 10
)
$ErrorActionPreference = "Stop"
if (-not $env:LOCALAPPDATA -or -not $env:APPDATA) { throw "LOCALAPPDATA ou APPDATA n'est pas defini." }
$dest = Join-Path $env:LOCALAPPDATA "Programs\Rocksy"
$exe = Join-Path $dest "rocksy.exe"
$lnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Rocksy.lnk"
$classes = "Software\Classes"
$progId = "Rocksy.Markdown"
$extensions = ".md", ".markdown"
$hkcu = [Microsoft.Win32.Registry]::CurrentUser

# --- PATH (pure string functions: no side effect) ---------------------------

function Get-NormalizedPathEntry([string]$Entry) {
  return $Entry.Trim().Trim('"').TrimEnd('\')
}

# Returns $PathValue with $Entry appended, or $PathValue unchanged if an
# equivalent entry is already there (case, trailing backslash, quotes and
# %VAR% forms are ignored for the comparison).
function Add-PathEntry([string]$PathValue, [string]$Entry) {
  $wanted = Get-NormalizedPathEntry $Entry
  foreach ($segment in $PathValue.Split(';')) {
    $plain = Get-NormalizedPathEntry $segment
    $expanded = Get-NormalizedPathEntry ([Environment]::ExpandEnvironmentVariables($plain))
    if ($plain -ieq $wanted -or $expanded -ieq $wanted) { return $PathValue }
  }
  if ($PathValue.Trim(' ;').Length -eq 0) { return $Entry }
  return $PathValue.TrimEnd(';') + ';' + $Entry
}

# Returns $PathValue without the entries equal to $Entry (the literal folder,
# any case, with or without trailing backslash or quotes). Every other entry
# is kept as written, in the same order.
function Remove-PathEntry([string]$PathValue, [string]$Entry) {
  $wanted = Get-NormalizedPathEntry $Entry
  $kept = @($PathValue.Split(';') | Where-Object { (Get-NormalizedPathEntry $_) -ine $wanted })
  return ($kept -join ';')
}

# Applies $Edit (Add-PathEntry or Remove-PathEntry) to the user PATH. The
# value is read and written unexpanded, and its registry type is kept
# (REG_EXPAND_SZ for a new value), so entries such as %USERPROFILE%\bin survive.
function Update-UserPath([scriptblock]$Edit) {
  $key = $hkcu.CreateSubKey("Environment")   # opens the key, creates it only if absent
  try {
    $raw = $key.GetValue("Path", $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
    if ($null -ne $raw) {
      $kind = $key.GetValueKind("Path")
      if ($raw -isnot [string]) {
        Write-Warning "PATH utilisateur d'un type inattendu ($kind) : il n'est pas modifie."
        return $false
      }
    }
    $current = [string]$raw
    $updated = [string](& $Edit $current)
    if ($updated -ceq $current) { return $false }
    if ($updated.Length -eq 0) { $key.DeleteValue("Path", $false) } else { $key.SetValue("Path", $updated, $kind) }
  } finally { $key.Close() }
  # Tell running programs (Explorer, hence new terminals) that the environment
  # changed: removing a variable that does not exist writes nothing, but .NET
  # broadcasts WM_SETTINGCHANGE afterwards.
  try { [Environment]::SetEnvironmentVariable("ROCKSY_INSTALL_NOTIFY", $null, "User") } catch { }
  return $true
}

# --- Registry (HKCU only) -----------------------------------------------------

# Sets one value. CreateSubKey opens an existing key without touching its
# other values or subkeys (unlike `New-Item -Force`, which recreates the key).
function Set-RegistryValue([string]$SubKey, [string]$Name, $Value, [Microsoft.Win32.RegistryValueKind]$Kind = [Microsoft.Win32.RegistryValueKind]::String) {
  $key = $hkcu.CreateSubKey($SubKey)
  try { $key.SetValue($Name, $Value, $Kind) } finally { $key.Close() }
}

# Removes Rocksy's value from <ext>\OpenWithProgids, then the keys this script
# may have created, only if nothing else is left in them.
function Remove-OpenWithEntry([string]$Extension) {
  $key = $hkcu.OpenSubKey("$classes\$Extension\OpenWithProgids", $true)
  if ($null -eq $key) { return }
  try { $key.DeleteValue($progId, $false) } finally { $key.Close() }
  foreach ($sub in "$classes\$Extension\OpenWithProgids", "$classes\$Extension") {
    $key = $hkcu.OpenSubKey($sub)
    if ($null -eq $key) { continue }
    try { $empty = ($key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0) } finally { $key.Close() }
    if (-not $empty) { break }
    $hkcu.DeleteSubKey($sub, $false)
  }
}

# --- Running instance ----------------------------------------------------------

# Closes the installed Rocksy (the only one that locks the installed files; a
# copy run from another folder is left alone). Asks politely, waits, and stops
# the script if it is still there, unless -ForceClose.
function Close-InstalledRocksy {
  $running = @(Get-Process -Name rocksy -ErrorAction SilentlyContinue | Where-Object { $_.Path -and ($_.Path -ieq $exe) })
  if ($running.Count -eq 0) { return }
  if ($ForceClose) {
    Write-Host "Arret force de Rocksy."
    $running | Stop-Process -Force
  } else {
    Write-Host "Rocksy est ouvert : demande de fermeture (attente de $CloseTimeoutSec s au plus)..."
    foreach ($proc in $running) { try { [void]$proc.CloseMainWindow() } catch { } }
  }
  $deadline = (Get-Date).AddSeconds($CloseTimeoutSec)
  foreach ($proc in $running) {
    $left = [int][Math]::Max(0, ($deadline - (Get-Date)).TotalMilliseconds)
    try { [void]$proc.WaitForExit($left) } catch { }
  }
  $still = @($running | Where-Object { -not $_.HasExited })
  if ($still.Count -gt 0) {
    throw "Rocksy est encore ouvert. Rien n'a ete modifie : enregistre ton travail, ferme Rocksy, puis relance ce script (ou ajoute -ForceClose pour l'arreter de force, au risque de perdre les modifications non enregistrees)."
  }
  Start-Sleep -Milliseconds 300   # lets Windows release the file locks
}

# --- Uninstall -----------------------------------------------------------------

if ($Uninstall) {
  Close-InstalledRocksy
  if (Test-Path -LiteralPath $dest) { Remove-Item -LiteralPath $dest -Recurse -Force -ErrorAction SilentlyContinue }
  if (Test-Path -LiteralPath $lnk) { Remove-Item -LiteralPath $lnk -Force -ErrorAction SilentlyContinue }
  $hkcu.DeleteSubKeyTree("$classes\$progId", $false)
  $hkcu.DeleteSubKeyTree("$classes\Applications\rocksy.exe", $false)
  foreach ($ext in $extensions) { Remove-OpenWithEntry $ext }
  if (Update-UserPath { param($p) Remove-PathEntry $p $dest }) { "Dossier retire du PATH utilisateur." }
  if (Test-Path -LiteralPath $dest) { Write-Warning "Le dossier $dest n'a pas pu etre supprime entierement (fichier en cours d'utilisation ?). Supprime-le a la main." }
  "Rocksy desinstalle."
  "Les donnees de l'application sont conservees dans $(Join-Path $env:APPDATA 'dev.rocksy.app') (elles contiennent l'index, donc le texte des notes). Supprime ce dossier a la main pour tout effacer."
  return
}

# --- Install or update -----------------------------------------------------------

$src = $PSScriptRoot
foreach ($f in "rocksy.exe", "WebView2Loader.dll") {
  if (-not (Test-Path -LiteralPath (Join-Path $src $f) -PathType Leaf)) { throw "Fichier manquant a cote du script : $f" }
}
Close-InstalledRocksy
if (-not (Test-Path -LiteralPath $dest)) { New-Item -ItemType Directory -Path $dest | Out-Null }
foreach ($f in "rocksy.exe", "WebView2Loader.dll") {
  try { Copy-Item -LiteralPath (Join-Path $src $f) -Destination $dest -Force }
  catch { throw "Copie de $f impossible dans $dest (Rocksy est-il encore ouvert ?) : $($_.Exception.Message)" }
}

$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk); $s.TargetPath = $exe; $s.WorkingDirectory = $dest; $s.Description = "Rocksy"; $s.Save()

if ($exe.Contains('%')) {
  # In a shell command line "%" introduces a parameter (%1, %L...): such a path cannot be registered safely.
  Write-Warning "Le chemin d'installation contient '%' : Rocksy n'est pas declare dans 'Ouvrir avec'."
} else {
  # A Windows path cannot contain a double quote, so quoting $exe is enough.
  $cmd = '"' + $exe + '" "%1"'
  Set-RegistryValue "$classes\$progId" "" "Document Markdown"
  Set-RegistryValue "$classes\$progId\shell\open\command" "" $cmd
  Set-RegistryValue "$classes\$progId\DefaultIcon" "" "$exe,0"
  Set-RegistryValue "$classes\Applications\rocksy.exe\shell\open\command" "" $cmd
  foreach ($ext in $extensions) {
    Set-RegistryValue "$classes\Applications\rocksy.exe\SupportedTypes" $ext ""
    # Adds Rocksy's value only: the entries of other applications stay.
    Set-RegistryValue "$classes\$ext\OpenWithProgids" $progId ([byte[]]@()) ([Microsoft.Win32.RegistryValueKind]::None)
  }
}

if ($AddToPath) {
  if ($dest.Contains(';')) {
    Write-Warning "Le chemin d'installation contient ';' : il n'est pas ajoute au PATH."
  } elseif (Update-UserPath { param($p) Add-PathEntry $p $dest }) {
    "Dossier ajoute au PATH utilisateur (pris en compte dans un nouveau terminal)."
  } else {
    "Le PATH utilisateur contient deja le dossier."
  }
}
"Rocksy installe dans $dest"
"Double-clic sur les .md : clic droit sur un fichier .md > Ouvrir avec > Choisir une autre application > Rocksy > Toujours."
