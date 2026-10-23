param([ValidateSet('Download','Run')][string]$Mode='Download',[switch]$DefenderScanPassed)
$ErrorActionPreference='Stop'
$root=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$corpus=Join-Path $root 'corpus'
$results=Join-Path $root 'corpus-results'
New-Item -ItemType Directory -Force $corpus,$results | Out-Null
$limits=@('--rm','--user','65532:65532','--cap-drop','ALL','--security-opt','no-new-privileges','--cpus','2','--memory','4g','--pids-limit','128','--read-only','--tmpfs','/tmp:rw,noexec,nosuid,size=128m')
if($Mode -eq 'Download') {
    docker run @limits --mount "type=bind,source=$corpus,target=/corpus" pdf-editor-corpus:local python3 /opt/pdf-editor/download.py
} else {
    if(-not $DefenderScanPassed) { throw 'A completed clean Microsoft Defender scan is required before running the corpus.' }
    docker run @limits --network none --mount "type=bind,source=$corpus,target=/corpus,readonly" --mount "type=bind,source=$results,target=/results" pdf-editor-corpus:local
}
if($LASTEXITCODE -ne 0) { throw 'Corpus container failed.' }
