$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$release = 'chromium/8111'
$sha256 = '3452d11031050ec65e57f180215b57b9e6d465e3fbedafa8a77ad90885c7d23e'
$cache = Join-Path $repo 'target/pdfium'
New-Item -ItemType Directory -Force $cache | Out-Null
$archive = Join-Path $cache 'pdfium-win-x64.tgz'
if (-not (Test-Path -LiteralPath $archive)) {
    Invoke-WebRequest "https://github.com/bblanchon/pdfium-binaries/releases/download/$release/pdfium-win-x64.tgz" -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sha256) {
    throw 'PDFium archive checksum mismatch.'
}
# Extract only the known library and license directory from the verified archive.
tar -xzf $archive -C $cache bin/pdfium.dll LICENSE licenses
if ($LASTEXITCODE -ne 0) { throw 'PDFium extraction failed.' }
foreach ($profile in @('debug', 'release')) {
    $destination = Join-Path $repo "target/$profile"
    New-Item -ItemType Directory -Force $destination | Out-Null
    $sourceDll = Join-Path $cache 'bin/pdfium.dll'
    $destinationDll = Join-Path $destination 'pdfium.dll'
    # Leave an identical DLL alone, including when an editor has it loaded.
    if (-not (Test-Path -LiteralPath $destinationDll) -or
        (Get-FileHash -LiteralPath $sourceDll).Hash -ne (Get-FileHash -LiteralPath $destinationDll).Hash) {
        Copy-Item -LiteralPath $sourceDll -Destination $destinationDll
    }
    Copy-Item -LiteralPath (Join-Path $cache 'LICENSE') -Destination (Join-Path $destination 'pdfium-LICENSE.txt')
    $notices = Join-Path $destination 'pdfium-licenses'
    New-Item -ItemType Directory -Force $notices | Out-Null
    Get-ChildItem -LiteralPath (Join-Path $cache 'licenses') | Copy-Item -Destination $notices -Recurse -Force
}
