<#
.SYNOPSIS
  KPIs du vault Obsidian pour dimensionner l'index (lecture seule). A lancer dans PowerShell sous Windows.

.DESCRIPTION
  Parcourt le vault en ignorant les dossiers a point (comme Obsidian), lit les .md en memoire
  et ecrit UNIQUEMENT des compteurs et des statistiques dans un JSON. Aucun contenu de note,
  aucun nom de tag, de propriete ou de lien n'est ecrit. Aucune ecriture dans le vault.

  Mesure : fichiers/tailles, distribution des .md (mediane, p95, max), mots/caracteres,
  wikilinks/embeds/liens non resolus, degres du graphe, tags, frontmatter, headings,
  taches, .excalidraw.md, chemins longs, rythme de creation, taille du cache IndexedDB d'Obsidian.

.EXAMPLE
  .\vault-kpis.ps1
  .\vault-kpis.ps1 -Vault "G:\Mon Drive\MonVault" -OutJson .\vault-kpis.json

.NOTES
  Sur Google Drive a froid, la lecture des .md peut etre lente : lance-le apres un premier
  parcours du vault (ou accepte la duree, elle est mesuree et reportee dans le JSON).
#>
param(
  [string]$Vault = "G:\Mon Drive\MonVault",
  [string]$OutJson = "$PSScriptRoot\..\data\vault-kpis.json"
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Vault)) { throw "Vault introuvable : $Vault" }
$root = (Resolve-Path $Vault).Path.TrimEnd('\')

function Percentile($sorted, [double]$p) {
  if ($sorted.Count -eq 0) { return $null }
  $idx = [int][math]::Ceiling($p * $sorted.Count) - 1
  $sorted[[math]::Max(0, [math]::Min($idx, $sorted.Count - 1))]
}
function Stats($values) {
  $v = @($values | Sort-Object)
  if ($v.Count -eq 0) { return [ordered]@{ count = 0 } }
  $sum = ($v | Measure-Object -Sum).Sum
  [ordered]@{
    count = $v.Count; sum = $sum; mean = [math]::Round($sum / $v.Count, 1)
    median = Percentile $v 0.5; p95 = Percentile $v 0.95; max = $v[-1]
  }
}

# --- 1. Parcours (dossiers a point ignores) ---------------------------------
$swWalk = [Diagnostics.Stopwatch]::StartNew()
$files = New-Object System.Collections.Generic.List[IO.FileInfo]
$dirCount = 0
$stack = New-Object System.Collections.Generic.Stack[IO.DirectoryInfo]
$stack.Push([IO.DirectoryInfo]::new($root))
while ($stack.Count) {
  $d = $stack.Pop()
  foreach ($sub in $d.EnumerateDirectories()) {
    if ($sub.Name.StartsWith('.')) { continue }
    $dirCount++; $stack.Push($sub)
  }
  foreach ($f in $d.EnumerateFiles()) {
    if ($f.Name.StartsWith('.')) { continue }
    $files.Add($f)
  }
}
$swWalk.Stop()

$rel = @{}   # FullName -> chemin relatif avec /
foreach ($f in $files) { $rel[$f.FullName] = $f.FullName.Substring($root.Length + 1).Replace('\', '/') }

$md   = @($files | Where-Object { $_.Extension -ieq '.md' })
$attC = $files.Count - $md.Count

# Resolution des liens : noms de base (md sans extension, autres avec) et chemins relatifs
$byName = @{}; $byPath = @{}
foreach ($f in $files) {
  $r = $rel[$f.FullName]
  if ($f.Extension -ieq '.md') { $key = [IO.Path]::GetFileNameWithoutExtension($f.Name); $pk = $r.Substring(0, $r.Length - 3) }
  else { $key = $f.Name; $pk = $r }
  $byName[$key.ToLowerInvariant()] = $true
  $byPath[$pk.ToLowerInvariant()] = $true
}

# --- 2. Lecture des .md (en memoire, seuls les compteurs sont gardes) ------
$reFence    = [regex]'(?ms)^(```|~~~).*?^\1[^\r\n]*$'
$reInline   = [regex]'`[^`\r\n]+`'
$reWiki     = [regex]'(?<bang>!?)\[\[(?<t>[^\]\|#\^\r\n]+)(?:[#\^][^\]\|\r\n]*)?(?:\|[^\]\r\n]*)?\]\]'
$reTag      = [regex]'(?<![\w/&#\[\(])#(?<t>[\p{L}\p{N}_\-/]*\p{L}[\p{L}\p{N}_\-/]*)'
$reHeading  = [regex]'(?m)^#{1,6}\s+\S'
$reTask     = [regex]'(?m)^\s*[-*+]\s\[(?<s>[ xX])\]'
$reFmKey    = [regex]'(?m)^([A-Za-z0-9_\-]+)\s*:'
$reWord     = [regex]'\S+'

$mdBytes = @(); $words = @(); $chars = @(); $heads = @()
$outLinks = @(); $wikiTotal = 0; $embedTotal = 0
$unresolvedTotal = 0; $unresolvedTargets = @{}
$linkedFrom = @{}                 # cible (nom de base) -> nb de liens entrants
$tagOcc = 0; $tagSet = @{}; $notesWithTags = 0
$fmNotes = 0; $fmKeyCount = @(); $fmKeySet = @{}
$tasksOpen = 0; $tasksDone = 0
$excalidraw = 0; $emptyMd = 0
$readErrors = 0
$swRead = [Diagnostics.Stopwatch]::StartNew()

foreach ($f in $md) {
  try { $text = [IO.File]::ReadAllText($f.FullName, [Text.Encoding]::UTF8) }
  catch { $readErrors++; continue }
  if ($f.Name -like '*.excalidraw.md') { $excalidraw++ }
  $mdBytes += $f.Length
  if ($text.Length -eq 0) { $emptyMd++ }
  $chars += $text.Length
  $words += $reWord.Matches($text).Count

  # frontmatter
  $body = $text; $fmTags = 0
  if ($text.StartsWith("---")) {
    $m = [regex]::Match($text, '(?s)\A---[ \t]*\r?\n(?<fm>.*?)\r?\n---[ \t]*(\r?\n|\z)')
    if ($m.Success) {
      $fmNotes++
      $keys = $reFmKey.Matches($m.Groups['fm'].Value)
      $fmKeyCount += $keys.Count
      foreach ($k in $keys) { $fmKeySet[$k.Groups[1].Value.ToLowerInvariant()] = $true }
      if ($m.Groups['fm'].Value -match '(?m)^tags?\s*:') { $fmTags = 1 }
      $body = $text.Substring($m.Length)
    }
  }

  # on retire les blocs de code pour liens, tags et headings
  $clean = $reInline.Replace($reFence.Replace($body, ''), '')

  $heads += $reHeading.Matches($clean).Count
  foreach ($t in $reTask.Matches($clean)) { if ($t.Groups['s'].Value -eq ' ') { $tasksOpen++ } else { $tasksDone++ } }

  $tags = $reTag.Matches($clean)
  $tagOcc += $tags.Count
  foreach ($t in $tags) { $tagSet[$t.Groups['t'].Value.ToLowerInvariant()] = $true }
  if ($tags.Count -gt 0 -or $fmTags) { $notesWithTags++ }

  $n = 0
  foreach ($l in $reWiki.Matches($clean)) {
    $n++
    if ($l.Groups['bang'].Value -eq '!') { $embedTotal++ } else { $wikiTotal++ }
    $target = $l.Groups['t'].Value.Trim().ToLowerInvariant()
    if ($target.EndsWith('.md')) { $target = $target.Substring(0, $target.Length - 3) }
    $name = $target.Substring($target.LastIndexOf('/') + 1)
    $ok = if ($target.Contains('/')) { $byPath.ContainsKey($target) } else { $byName.ContainsKey($target) }
    if ($ok) { $linkedFrom[$name] = 1 + [int]$linkedFrom[$name] }
    else { $unresolvedTotal++; $unresolvedTargets[$target] = $true }
  }
  $outLinks += $n
}
$swRead.Stop()

# --- 3. Divers : chemins, croissance, cache Obsidian -----------------------
$pathLens = @($files | ForEach-Object { $_.FullName.Length })
$maxRelLen = ($rel.Values | ForEach-Object { $_.Length } | Measure-Object -Maximum).Maximum

function ByMonth($items, $prop) {
  $h = [ordered]@{}
  $items | Group-Object { $_.$prop.ToString('yyyy-MM') } | Sort-Object Name -Descending | Select-Object -First 24 |
    Sort-Object Name | ForEach-Object { $h[$_.Name] = $_.Count }
  $h
}

$idb = Join-Path $env:APPDATA 'obsidian\IndexedDB'
$idbBytes = $null
if (Test-Path $idb) { $idbBytes = (Get-ChildItem $idb -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum }

$inDeg = @($linkedFrom.Values)
$ext = [ordered]@{}
$files | Where-Object { $_.Extension -ine '.md' } | Group-Object { $_.Extension.ToLowerInvariant() } |
  Sort-Object Count -Descending | Select-Object -First 15 |
  ForEach-Object { $ext[$_.Name] = [ordered]@{ count = $_.Count; bytes = ($_.Group | Measure-Object Length -Sum).Sum } }

$result = [ordered]@{
  generatedAt = (Get-Date).ToString('s')
  note = 'Compteurs uniquement : aucun contenu de note, nom de tag, de propriete ou de lien.'
  scope = 'hors dossiers a point (vue Obsidian)'
  files = [ordered]@{
    total = $files.Count; markdown = $md.Count; attachments = $attC; folders = $dirCount
    totalBytes = ($files | Measure-Object Length -Sum).Sum
    attachmentsByExtension = $ext
  }
  markdown = [ordered]@{
    bytes = Stats $mdBytes
    words = Stats $words
    characters = Stats $chars
    emptyFiles = $emptyMd
    excalidrawMd = $excalidraw
    readErrors = $readErrors
  }
  structure = [ordered]@{
    headings = Stats $heads
    notesWithFrontmatter = $fmNotes
    frontmatterKeysPerNote = Stats $fmKeyCount
    distinctFrontmatterKeys = $fmKeySet.Count
    tasksOpen = $tasksOpen; tasksDone = $tasksDone
  }
  tags = [ordered]@{
    distinctInlineTags = $tagSet.Count; inlineOccurrences = $tagOcc; notesWithTags = $notesWithTags
  }
  links = [ordered]@{
    wikilinks = $wikiTotal; embeds = $embedTotal
    unresolvedOccurrences = $unresolvedTotal; unresolvedDistinctTargets = $unresolvedTargets.Count
    outgoingPerNote = Stats $outLinks
    incomingPerLinkedTarget = Stats $inDeg
    notesWithoutOutgoing = @($outLinks | Where-Object { $_ -eq 0 }).Count
    distinctLinkedTargets = $linkedFrom.Count
    caveat = 'Resolution par nom de base (homonymes non departages), liens Markdown [](...) non comptes.'
  }
  paths = [ordered]@{
    maxAbsoluteLength = ($pathLens | Measure-Object -Maximum).Maximum
    maxRelativeLength = $maxRelLen
    over240 = @($pathLens | Where-Object { $_ -gt 240 }).Count
  }
  growth = [ordered]@{
    caveat = 'Google Drive peut remettre CreationTime a la date de synchronisation : prends LastWriteTime avec prudence.'
    mdCreatedByMonth = ByMonth $md 'CreationTime'
    mdModifiedByMonth = ByMonth $md 'LastWriteTime'
  }
  obsidianCache = [ordered]@{ indexedDbBytes = $idbBytes; path = $idb }
  timingsMs = [ordered]@{ walk = $swWalk.ElapsedMilliseconds; readMd = $swRead.ElapsedMilliseconds }
}

$result | ConvertTo-Json -Depth 8 | Out-File $OutJson -Encoding utf8
Write-Host "KPIs ecrits dans $OutJson" -ForegroundColor Green
Write-Host ("{0} fichiers ({1} .md), {2} mots, {3} wikilinks ({4} non resolus), parcours {5} ms, lecture {6} ms" -f `
  $files.Count, $md.Count, ($words | Measure-Object -Sum).Sum, $wikiTotal, $unresolvedTotal, $swWalk.ElapsedMilliseconds, $swRead.ElapsedMilliseconds)
