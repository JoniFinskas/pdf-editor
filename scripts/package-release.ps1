param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = Split-Path $PSScriptRoot -Parent
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repo 'dist' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$manifest = Get-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Raw
$version = [regex]::Match($manifest, '(?m)^version = "([0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?)"').Groups[1].Value
if (-not $version) { throw 'Workspace version is unavailable.' }
$changes = & git -C $repo status --porcelain
if ($LASTEXITCODE -ne 0 -or $changes) { throw 'Package only a clean, committed checkout.' }
$commit = (& git -C $repo rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot read source commit.' }
$release = Join-Path $repo 'target/release'
$required = @('pdf-editor.exe','pdf-editor-cli.exe','pdfium.dll','pdfium-LICENSE.txt','pdfium-licenses')
foreach ($name in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $release $name))) { throw "Missing release input: $name" }
}
# Read only the console binary's version. Never create a console or GUI window.
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = Join-Path $release 'pdf-editor-cli.exe'
$start.Arguments = '--version'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$child = [Diagnostics.Process]::Start($start)
try {
    if (-not $child.WaitForExit(5000)) { $child.Kill(); throw 'Version check timed out.' }
    $actual = $child.StandardOutput.ReadToEnd().Trim()
    if ($child.ExitCode -ne 0 -or $actual -ne "PDF Editor $version") {
        throw 'Release binary version does not match Cargo.toml; rebuild the release.'
    }
} finally { $child.Dispose() }
$name = "pdf-editor-$version-windows-x64"
$folder = Join-Path $OutputDirectory $name
$archive = Join-Path $OutputDirectory "$name.zip"
if ((Test-Path -LiteralPath $folder) -or (Test-Path -LiteralPath $archive)) {
    throw 'This version is already packaged. Existing artifacts are never overwritten.'
}
New-Item -ItemType Directory -Path $folder -Force | Out-Null
foreach ($item in $required) { Copy-Item -LiteralPath (Join-Path $release $item) -Destination $folder -Recurse }
foreach ($item in @('LICENSE','README.md','MANUAL_TEST.md','TEST_REPORT.md')) {
    Copy-Item -LiteralPath (Join-Path $repo $item) -Destination $folder
}
Copy-Item -LiteralPath (Join-Path $repo 'app/assets/fonts/OFL.txt') -Destination (Join-Path $folder 'Inter-OFL.txt')
[ordered]@{ version=$version; source_commit=$commit; platform='windows-x64'; packaged_utc=[DateTime]::UtcNow.ToString('o') } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $folder 'build-info.json') -Encoding utf8
$checksums = Get-ChildItem -LiteralPath $folder -File -Recurse | Sort-Object FullName | ForEach-Object {
    $relative = [IO.Path]::GetRelativePath($folder,$_.FullName).Replace('\','/')
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(),$relative
}
$checksums | Set-Content -LiteralPath (Join-Path $folder 'SHA256SUMS') -Encoding utf8
Compress-Archive -LiteralPath $folder -DestinationPath $archive -CompressionLevel Optimal
$hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $name.zip" | Set-Content -LiteralPath "$archive.sha256" -Encoding utf8
Write-Output $archive
