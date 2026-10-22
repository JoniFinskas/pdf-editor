$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$release = 'chromium/8111'
$sha256 = '3452d11031050ec65e57f180215b57b9e6d465e3fbedafa8a77ad90885c7d23e'
$cache = Join-Path $repo 'target/pdfium'
New-Item -ItemType Directory -Force $cache | Out-Null
$archive = Join-Path $cache 'pdfium-win-x64.tgz'
Invoke-WebRequest "https://github.com/bblanchon/pdfium-binaries/releases/download/$release/pdfium-win-x64.tgz" -OutFile $archive
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sha256) {
    throw 'PDFium archive checksum mismatch.'
}
# Extract only the known library and license directory from the verified archive.
tar -xzf $archive -C $cache bin/pdfium.dll LICENSE
if ($LASTEXITCODE -ne 0) { throw 'PDFium extraction failed.' }
foreach ($profile in @('debug', 'release')) {
    $destination = Join-Path $repo "target/$profile"
    New-Item -ItemType Directory -Force $destination | Out-Null
    Copy-Item -LiteralPath (Join-Path $cache 'bin/pdfium.dll') -Destination $destination
    Copy-Item -LiteralPath (Join-Path $cache 'LICENSE') -Destination (Join-Path $destination 'pdfium-LICENSE.txt')
}
