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
  Mesures ajoutees (meme passe de lecture) : blocs Mermaid par type, callouts par type, tableaux,
  tags en ligne / frontmatter / hierarchiques (compteurs et profondeur, jamais les noms), embeds par
  extension, images Markdown, blocs de code par langage (top 10), taches.

.EXAMPLE
  .\vault-kpis.ps1
  .\vault-kpis.ps1 -Vault "D:\chemin\vers\mon-vault"
  .\vault-kpis.ps1 -Vault "D:\chemin\vers\mon-vault" -OutFile .\mes-kpis.json

.NOTES
  Sur Google Drive a froid, la lecture des .md peut etre lente : lance-le apres un premier
  parcours du vault (ou accepte la duree, elle est mesuree et reportee dans le JSON).
#>
param(
  [string]$Vault,
  [Alias('OutJson')]
  [string]$OutFile = ("$PSScriptRoot\..\data\vault-kpis-{0}.json" -f (Get-Date -Format 'yyyy-MM-dd'))
)
if (-not $Vault) { $Vault = Read-Host 'Chemin du vault Obsidian (dossier qui contient .obsidian)' }
$Vault = $Vault.Trim().Trim('"')
if (-not $Vault) { throw 'Aucun chemin de vault donne.' }

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Vault)) { throw "Vault introuvable : $Vault" }
$root = (Resolve-Path $Vault).Path.TrimEnd('\')

# Ne jamais ecraser la mesure de reference
$outDir = Split-Path -Parent $OutFile
if ($outDir -and -not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }
if ((Split-Path -Leaf $OutFile) -ieq 'vault-kpis.json') { throw "Refus d'ecraser vault-kpis.json : choisis un autre -OutFile." }

# Incremente un compteur dans une table de hachage
function Inc($h, $k) { $h[$k] = 1 + [int]$h[$k] }
# Distribution triee par compte decroissant (top N, le reste regroupe dans '(autres)')
function Dist($h, [int]$top = 0) {
  $o = [ordered]@{}; $i = 0; $rest = 0
  foreach ($e in ($h.GetEnumerator() | Sort-Object @{Expression='Value';Descending=$true}, @{Expression='Key'})) {
    if ($top -gt 0 -and $i -ge $top) { $rest += $e.Value } else { $o[[string]$e.Key] = $e.Value }
    $i++
  }
  if ($rest -gt 0) { $o['(autres)'] = $rest }
  $o
}

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
# --- nouvelles mesures (tolerantes au CRLF) ---
$reFenceN   = [regex]'(?ms)^[ \t]{0,3}(?<f>`{3,}|~{3,})(?<info>[^\r\n]*)\r?\n(?<code>.*?)^[ \t]{0,3}\k<f>[ \t]*\r?$'
$reCallout  = [regex]'(?m)^[ \t]*(?:>[ \t]*)+\[!(?<t>[^\]\r\n\s]+)\]'
$reTable    = [regex]'(?m)^[^\r\n]*\|[^\r\n]*\r?\n(?=[^\r\n]*\|)[ \t]*\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*\r?$'
$reTagN     = [regex]'(?m)(?<=^|\s)#(?<t>[\p{L}\p{N}_\-/]+)'
$reEmbed    = [regex]'!\[\[(?<t>[^\]\|#\^\r\n]+)(?:[#\^][^\]\|\r\n]*)?(?:\|[^\]\r\n]*)?\]\]'
$reMdImg    = [regex]'!\[[^\]\r\n]*\]\((?<u>[^)\r\n]+)\)'
$reFmTagKey = [regex]'(?m)^tags?[ \t]*:(?<v>[^\r\n]*)$'
$reTaskN    = [regex]'(?m)^[ \t]*[-*+][ \t]\[(?<s>[ xX])\]'
$otherExt   = @{}
foreach ($e in 'gif','svg','webp','bmp','avif','mp3','wav','ogg','m4a','flac','mp4','webm','mov','mkv','canvas','excalidraw','docx','xlsx','pptx','txt','json','csv','zip','base') { $otherExt[$e] = $true }

$mmCount = 0; $mmNotes = 0; $mmMax = 0; $mmByType = @{}
$coCount = 0; $coNotes = 0; $coByType = @{}
$tbCount = 0; $tbNotes = 0
$tgInline = 0; $tgFm = 0; $tgSet = @{}; $tgSetInline = @{}; $tgSetFm = @{}; $tgNotesInline = 0; $tgNotesFm = 0
$tgOccHier = 0; $tgDepth = @{}
$emByExt = @{}; $emTotal = 0; $mdImgLocal = 0; $mdImgRemote = 0
$cbTotal = 0; $cbLang = @{}
$tkOpen = 0; $tkDone = 0

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
  $body = $text; $fmTags = 0; $fmText = $null
  if ($text.StartsWith("---")) {
    $m = [regex]::Match($text, '(?s)\A---[ \t]*\r?\n(?<fm>.*?)\r?\n---[ \t]*(\r?\n|\z)')
    if ($m.Success) {
      $fmNotes++
      $fmText = $m.Groups['fm'].Value
      $keys = $reFmKey.Matches($m.Groups['fm'].Value)
      $fmKeyCount += $keys.Count
      foreach ($k in $keys) { $fmKeySet[$k.Groups[1].Value.ToLowerInvariant()] = $true }
      if ($m.Groups['fm'].Value -match '(?m)^tags?\s*:') { $fmTags = 1 }
      $body = $text.Substring($m.Length)
    }
  }

  # on retire les blocs de code pour liens, tags et headings
  $clean = $reInline.Replace($reFence.Replace($body, ''), '')

  # ---- nouvelles mesures : blocs de code et Mermaid (sur le corps, avant retrait du code) ----
  $mmHere = 0
  foreach ($fm in $reFenceN.Matches($body)) {
    $cbTotal++
    $lang = ($fm.Groups['info'].Value.Trim() -split '[\s{]')[0].TrimStart('.').ToLowerInvariant()
    if (-not $lang) { $lang = '(aucun)' }
    Inc $cbLang $lang
    if ($lang -eq 'mermaid') {
      $mmHere++
      $kind = ""; $inFm = $false
      foreach ($ln in ($fm.Groups['code'].Value -split '\r?\n')) {
        $l2 = $ln.Trim()
        if ($l2 -eq '') { continue }
        if ($l2 -eq '---') { $inFm = -not $inFm; continue }
        if ($inFm -or $l2.StartsWith('%%')) { continue }
        $kind = ($l2 -split '[\s;{]')[0].ToLowerInvariant(); break
      }
      $mt = switch -Regex ($kind) {
        '^(flowchart|graph)'   { 'flowchart/graph'; break }
        '^sequencediagram$'    { 'sequenceDiagram'; break }
        '^classdiagram'        { 'classDiagram'; break }
        '^statediagram'        { 'stateDiagram'; break }
        '^erdiagram$'          { 'erDiagram'; break }
        '^gantt$'              { 'gantt'; break }
        '^pie$'                { 'pie'; break }
        '^mindmap$'            { 'mindmap'; break }
        '^wardley-beta$'       { 'wardley-beta'; break }
        default                { 'autre' }
      }
      Inc $mmByType $mt
    }
  }
  if ($mmHere -gt 0) { $mmNotes++; $mmCount += $mmHere; if ($mmHere -gt $mmMax) { $mmMax = $mmHere } }

  # corps sans code (bloc ou en ligne), tolerant au CRLF : base des autres nouveaux compteurs
  $cn = $reInline.Replace($reFenceN.Replace($body, ''), '')

  $c = $reCallout.Matches($cn)
  if ($c.Count -gt 0) { $coNotes++; $coCount += $c.Count; foreach ($x in $c) { Inc $coByType $x.Groups['t'].Value.ToLowerInvariant() } }

  $tb = $reTable.Matches($cn).Count
  if ($tb -gt 0) { $tbNotes++; $tbCount += $tb }

  foreach ($x in $reTaskN.Matches($cn)) { if ($x.Groups['s'].Value -eq ' ') { $tkOpen++ } else { $tkDone++ } }

  # tags en ligne (regles Obsidian : # apres debut de ligne ou espace, pas purement numerique)
  $inlineHere = 0
  foreach ($x in $reTagN.Matches($cn)) {
    $tn = $x.Groups['t'].Value.Trim('/').ToLowerInvariant()
    if ($tn -notmatch '\D') { continue }
    $inlineHere++; $tgSet[$tn] = $true; $tgSetInline[$tn] = $true
    $dp = ($tn -split '/').Count; Inc $tgDepth ([string]$dp)
    if ($dp -gt 1) { $tgOccHier++ }
  }
  $tgInline += $inlineHere
  if ($inlineHere -gt 0) { $tgNotesInline++ }

  # tags du frontmatter (tags:/tag: en ligne, [a, b] ou liste YAML)
  $fmHere = 0
  if ($fmText) {
    foreach ($km in $reFmTagKey.Matches($fmText)) {
      $items = New-Object System.Collections.Generic.List[string]
      foreach ($p in ($km.Groups['v'].Value -split '[,\s]+')) { $items.Add($p) }
      $after = $fmText.Substring($km.Index + $km.Length) -split '\r?\n'
      foreach ($ln in $after) {
        if ($ln -match '^\s*-\s*(?<i>.+?)\s*$') { $items.Add($Matches['i']) }
        elseif ($ln.Trim() -eq '') { continue }
        else { break }
      }
      foreach ($p in $items) {
        $tn = $p.Trim().Trim('[', ']', '"', "'", '#', '/').ToLowerInvariant()
        if ($tn -eq '' -or $tn -notmatch '\D') { continue }
        $fmHere++; $tgSet[$tn] = $true; $tgSetFm[$tn] = $true
        $dp = ($tn -split '/').Count; Inc $tgDepth ([string]$dp)
        if ($dp -gt 1) { $tgOccHier++ }
      }
    }
  }
  $tgFm += $fmHere
  if ($fmHere -gt 0) { $tgNotesFm++ }

  # embeds ![[...]] par extension de cible, images Markdown
  foreach ($x in $reEmbed.Matches($cn)) {
    $emTotal++
    $tg = $x.Groups['t'].Value.Trim().ToLowerInvariant()
    $ex = [IO.Path]::GetExtension($tg).TrimStart('.')
    $cat = if ($ex -eq 'png') { 'png' } elseif ($ex -eq 'jpg' -or $ex -eq 'jpeg') { 'jpg' } elseif ($ex -eq 'pdf') { 'pdf' }
           elseif ($ex -eq '' -or $ex -eq 'md' -or -not $otherExt.ContainsKey($ex)) { 'md/sans extension' } else { 'autre' }
    Inc $emByExt $cat
  }
  foreach ($x in $reMdImg.Matches($cn)) { if ($x.Groups['u'].Value.Trim() -match '^[a-zA-Z][a-zA-Z0-9+.\-]*://') { $mdImgRemote++ } else { $mdImgLocal++ } }

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
  mermaid = [ordered]@{
    blocks = $mmCount; notesWithMermaid = $mmNotes; maxBlocksInOneNote = $mmMax
    byType = Dist $mmByType
  }
  callouts = [ordered]@{
    total = $coCount; notesWithCallouts = $coNotes; distinctTypes = $coByType.Count
    byType = Dist $coByType 20
  }
  tables = [ordered]@{ count = $tbCount; notesWithTables = $tbNotes }
  tagsDetail = [ordered]@{
    caveat = 'Noms de tags jamais ecrits. Occurrences en ligne hors code, frontmatter tags:/tag:, hierarchique = contient /.'
    inlineOccurrences = $tgInline; frontmatterOccurrences = $tgFm
    notesWithInlineTags = $tgNotesInline; notesWithFrontmatterTags = $tgNotesFm
    distinctTags = $tgSet.Count; distinctInline = $tgSetInline.Count; distinctFrontmatter = $tgSetFm.Count
    distinctHierarchical = @($tgSet.Keys | Where-Object { $_.Contains('/') }).Count
    hierarchicalOccurrences = $tgOccHier
    maxDepth = if ($tgDepth.Count) { ($tgDepth.Keys | ForEach-Object { [int]$_ } | Measure-Object -Maximum).Maximum } else { 0 }
    occurrencesByDepth = Dist $tgDepth
  }
  embedsDetail = [ordered]@{
    wikiEmbeds = $emTotal; byTarget = Dist $emByExt
    markdownImagesLocal = $mdImgLocal; markdownImagesRemote = $mdImgRemote
  }
  codeBlocks = [ordered]@{
    total = $cbTotal; distinctLanguages = $cbLang.Count; topLanguages = Dist $cbLang 10
  }
  tasks = [ordered]@{ open = $tkOpen; done = $tkDone; total = $tkOpen + $tkDone }
  obsidianCache = [ordered]@{ indexedDbBytes = $idbBytes; path = $idb }
  timingsMs = [ordered]@{ walk = $swWalk.ElapsedMilliseconds; readMd = $swRead.ElapsedMilliseconds }
}

$result | ConvertTo-Json -Depth 8 | Out-File $OutFile -Encoding utf8
Write-Host "KPIs ecrits dans $OutFile" -ForegroundColor Green
Write-Host ("{0} fichiers ({1} .md), {2} mots, {3} wikilinks ({4} non resolus), parcours {5} ms, lecture {6} ms" -f `
  $files.Count, $md.Count, ($words | Measure-Object -Sum).Sum, $wikiTotal, $unresolvedTotal, $swWalk.ElapsedMilliseconds, $swRead.ElapsedMilliseconds)
