<#
.SYNOPSIS
  Inventaire de l'API utilisee par chaque plugin communautaire (M1). Lecture seule, sous Windows.

.DESCRIPTION
  Lit .obsidian\plugins\*\main.js et compte, par plugin, des motifs utiles pour dimensionner le shim :
  classes et fonctions de "obsidian", chemins app.*, enregistrements (registerView, addCommand...),
  evenements ecoutes, modules CodeMirror, modules Node/Electron.
  La sortie ne contient que des noms de symboles d'API et des compteurs : pas de code, pas de
  valeur de data.json. Les bundles etant minifies, les chiffres sont des ordres de grandeur.

.EXAMPLE
  .\scan-plugin-api.ps1
  .\scan-plugin-api.ps1 -Vault "D:\chemin\vers\mon-vault" -OutJson .\plugin-api-usage.json
#>
param(
  [string]$Vault,
  [string]$OutJson = "$PSScriptRoot\..\data\plugin-api-usage.json"
)
if (-not $Vault) { $Vault = Read-Host 'Chemin du vault Obsidian (dossier qui contient .obsidian)' }
$Vault = $Vault.Trim().Trim('"')
if (-not $Vault) { throw 'Aucun chemin de vault donne.' }

$ErrorActionPreference = 'Stop'
$pluginsDir = Join-Path $Vault '.obsidian\plugins'
if (-not (Test-Path $pluginsDir)) { throw "Dossier des plugins introuvable : $pluginsDir" }

$obsidianSymbols = @('Plugin','PluginSettingTab','Setting','Modal','Notice','ItemView','View','MarkdownView','FileView',
  'TFile','TFolder','TAbstractFile','Menu','MenuItem','Component','Events','Scope','Keymap','MarkdownRenderer',
  'MarkdownRenderChild','WorkspaceLeaf','Workspace','Vault','FuzzySuggestModal','SuggestModal','AbstractInputSuggest',
  'EditorSuggest','ButtonComponent','TextComponent','ToggleComponent','DropdownComponent','SliderComponent',
  'getAllTags','parseYaml','stringifyYaml','parseFrontMatterEntry','parseFrontMatterTags','getFrontMatterInfo',
  'normalizePath','setIcon','addIcon','getIcon','requestUrl','request','Platform','debounce','moment','prepareFuzzySearch',
  'prepareSimpleSearch','sortSearchResults','renderResults','getLinkpath','resolveSubpath','loadPdfJs','loadMermaid',
  'sanitizeHTMLToDom','finishRenderMath','renderMath','htmlToMarkdown','editorInfoField','editorLivePreviewField',
  'editorViewField','livePreviewState','FileSystemAdapter','Editor','EditorPosition','MetadataCache','FileManager',
  'DataAdapter','Hotkey','Command','BaseComponent','Plugin_','ViewState','HoverPopover','Tasks')
$regs = @('registerView','registerEditorExtension','registerMarkdownPostProcessor','registerMarkdownCodeBlockProcessor',
  'registerEvent','registerInterval','registerDomEvent','registerExtensions','registerObsidianProtocolHandler',
  'registerHoverLinkSource','registerEditorSuggest','addCommand','addRibbonIcon','addStatusBarItem','addSettingTab',
  'loadData','saveData','onLayoutReady','onunload','onUserEnable','registerCliHandler')
$nodeMods = @('fs','path','os','child_process','electron','@electron/remote','crypto','http','https','net','url','util',
  'stream','zlib','buffer','events','worker_threads','process')

function Count($text, $pattern) { [regex]::Matches($text, $pattern).Count }
function TopCounts($matches, [int]$limit = 40) {
  $h = [ordered]@{}
  $matches | Group-Object | Sort-Object Count -Descending | Select-Object -First $limit |
    ForEach-Object { $h[$_.Name] = $_.Count }
  $h
}

$result = [ordered]@{
  generatedAt = (Get-Date).ToString('s')
  note = 'Symboles d API et compteurs uniquement. Bundles minifies : ordres de grandeur.'
  plugins = [ordered]@{}
}

foreach ($dir in Get-ChildItem $pluginsDir -Directory) {
  $mainJs = Join-Path $dir.FullName 'main.js'
  if (-not (Test-Path $mainJs)) { continue }
  $t = [IO.File]::ReadAllText($mainJs, [Text.Encoding]::UTF8)

  $obs = [ordered]@{}
  foreach ($s in $obsidianSymbols) {
    $n = Count $t "\b$s\b"
    if ($n -gt 0) { $obs[$s] = $n }
  }
  $reg = [ordered]@{}
  foreach ($s in $regs) { $n = Count $t "\b$s\b"; if ($n -gt 0) { $reg[$s] = $n } }
  $node = [ordered]@{}
  foreach ($m in $nodeMods) {
    $n = Count $t ('require\(\s*"(node:)?' + [regex]::Escape($m) + '"\s*\)')
    if ($n -gt 0) { $node[$m] = $n }
  }
  $node['window.require'] = Count $t 'window\.require'
  if ($node['window.require'] -eq 0) { $node.Remove('window.require') }

  $appPaths = [regex]::Matches($t, '\bapp\.(\w+)(?:\.(\w+))?') | ForEach-Object {
    if ($_.Groups[2].Success) { "app.$($_.Groups[1].Value).$($_.Groups[2].Value)" } else { "app.$($_.Groups[1].Value)" } }
  $events = [regex]::Matches($t, '\.on\(\s*"([a-z][a-z0-9\-]*)"') | ForEach-Object { $_.Groups[1].Value }
  $cm = [regex]::Matches($t, 'require\(\s*"(@codemirror/\w+|@lezer/\w+)"\s*\)') | ForEach-Object { $_.Groups[1].Value }

  $result.plugins[$dir.Name] = [ordered]@{
    mainJsKB = [math]::Round((Get-Item $mainJs).Length / 1KB)
    obsidianSymbols = $obs
    registrations = $reg
    events = TopCounts $events 25
    appPaths = TopCounts $appPaths 40
    codemirrorModules = TopCounts $cm
    nodeElectron = $node
    usesWorker = (Count $t 'new Worker\(') -gt 0
  }
  Write-Host ("{0,-35} {1,6} Ko  obsidian:{2,3}  app.*:{3,3}  node:{4}" -f $dir.Name, $result.plugins[$dir.Name].mainJsKB,
    $obs.Count, @($appPaths | Select-Object -Unique).Count, ($node.Keys -join ','))
}

$result | ConvertTo-Json -Depth 8 | Out-File $OutJson -Encoding utf8
Write-Host "Ecrit : $OutJson" -ForegroundColor Green
