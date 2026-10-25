param([ValidateSet('pdf-editor', 'pdf-editor-cli')][string]$Binary)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = Split-Path $PSScriptRoot -Parent
$previous = $env:CARGO_ENCODED_RUSTFLAGS
if ($env:RUSTFLAGS -or $previous) {
    throw 'Run the portable build without custom Rust flags.'
}
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$roots = @($repo, $env:USERPROFILE, $cargoHome) | Where-Object { $_ } | ForEach-Object { [IO.Path]::GetFullPath($_) }
$prefixes = foreach ($root in $roots) {
    $root
    $root.Replace('\','/')
    "\\?\$root"
    "//?/$($root.Replace('\','/'))"
}
$flags = $prefixes | Select-Object -Unique | ForEach-Object { "--remap-path-prefix=$_=/build" }
try {
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
    Push-Location $repo
    try {
        $arguments = @('build', '--release', '--locked', '--offline', '-j', '1')
        if ($Binary) { $arguments += @('--bin', $Binary) }
        else { $arguments += '--workspace' }
        & cargo @arguments
        if ($LASTEXITCODE -ne 0) { throw 'Portable release build failed.' }
    } finally { Pop-Location }
} finally { $env:CARGO_ENCODED_RUSTFLAGS = $previous }
