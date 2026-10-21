# Checks that every DLL the engines import is shipped, provided by Windows, or installed by First Run:
# downloads the pinned archives with the app's own downloader into an empty root, then walks the import
# tables through setup/dll_imports.rs and platform/dll_imports.rs. Run after changing an engine/CUDA pin in setup/mod.rs.
param(
    [Parameter(Mandatory = $true)][string]$StagingRoot
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $StagingRoot | Out-Null
$env:SPEECH_DLL_STAGING = (Resolve-Path $StagingRoot).Path
Push-Location (Join-Path $PSScriptRoot '..')
try {
    cargo test -p speech-server --lib dll_imports::tests::downloaded_runtime_can_be_loaded -- --ignored --nocapture
    if ($LASTEXITCODE -ne 0) { throw "DLL import check failed (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}
