# Scan downloaded corpus bytes without opening PDFs in a host application.
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$CorpusPath)
$ErrorActionPreference = 'Stop'
$corpus = (Resolve-Path -LiteralPath $CorpusPath).Path
if (-not (Test-Path -LiteralPath $corpus -PathType Container)) { throw 'Corpus folder is required.' }
if ((Get-Item -LiteralPath $corpus).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Corpus folder must not be a reparse point.' }
$platform = Join-Path $env:ProgramData 'Microsoft/Windows Defender/Platform'
$scanner = Get-ChildItem -LiteralPath $platform -Directory | Sort-Object Name -Descending | ForEach-Object {
    $candidate = Join-Path $_.FullName 'MpCmdRun.exe'
    if (Test-Path -LiteralPath $candidate -PathType Leaf) { $candidate }
} | Select-Object -First 1
if (-not $scanner) { throw 'Microsoft Defender scanner is unavailable; corpus processing is blocked.' }
$root = Split-Path $PSScriptRoot -Parent
$logFolder = Join-Path $root 'target/corpus-scan'
New-Item -ItemType Directory -Force -Path $logFolder | Out-Null
$log = Join-Path $logFolder 'defender.log'
# Native output can contain file names. Keep it in ignored local output; never echo it.
& $scanner -Scan -ScanType 3 -File $corpus -DisableRemediation *> $log
$code = $LASTEXITCODE
if ($code -ne 0) { throw "Microsoft Defender scan did not pass (exit $code); corpus processing is blocked." }
Write-Output 'Microsoft Defender custom scan passed (exit 0, remediation disabled).'
