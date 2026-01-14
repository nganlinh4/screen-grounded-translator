# Re-patch egui-snarl to ensure custom scroll-to-zoom is applied
Write-Host "Setting up patched egui-snarl..." -ForegroundColor Cyan
$snarlDir = Join-Path $PSScriptRoot "libs\egui-snarl"
if (Test-Path $snarlDir) {
    Remove-Item $snarlDir -Recurse -Force
}
& (Join-Path $PSScriptRoot "scripts\setup-egui-snarl.ps1")

# --- Build PromptDJ Frontend ---
Write-Host "Building PromptDJ Frontend..." -ForegroundColor Cyan
$pdjDir = Join-Path $PSScriptRoot "promptdj-midi"
$pdjDist = Join-Path $pdjDir "dist"
$pdjTargetDist = Join-Path $PSScriptRoot "src\overlay\prompt_dj\dist"

Push-Location $pdjDir
try {
    npm run build
}
finally {
    Pop-Location
}

if (Test-Path $pdjDist) {
    if (-not (Test-Path $pdjTargetDist)) {
        New-Item -ItemType Directory -Path $pdjTargetDist -Force | Out-Null
    }
    Copy-Item -Path "$pdjDist\*" -Destination $pdjTargetDist -Recurse -Force
    Write-Host "PromptDJ assets synchronized." -ForegroundColor Green
}
else {
    Write-Host "FAILED: PromptDJ build did not produce dist folder." -ForegroundColor Red
    exit 1
}

# --- Continue Main Build ---
# Extract version from Cargo.toml
$cargoContent = Get-Content "Cargo.toml" -Raw
if ($cargoContent -match 'version\s*=\s*"([^"]+)"') {
    $version = $matches[1]
}
else {
    Write-Host "Failed to extract version from Cargo.toml" -ForegroundColor Red
    exit 1
}

# Output paths
$outputExeName = "ScreenGoatedToolbox_v$version.exe"
$outputPath = "target/release/$outputExeName"
$exePathRelease = "target/release/screen-goated-toolbox.exe"

# =============================================================================
# Build Release version (LTO optimized + stripped)
# =============================================================================
Write-Host ""
Write-Host "=== Building ScreenGoatedToolbox v$version ===" -ForegroundColor Cyan
Write-Host "Using 'release' profile (LTO + stripped)..." -ForegroundColor Gray
cargo build --release

if (Test-Path $exePathRelease) {
    if (Test-Path $outputPath) {
        Remove-Item $outputPath
    }
    Move-Item $exePathRelease $outputPath
    $size = (Get-Item $outputPath).Length / 1MB
    Write-Host "  -> Created: $outputExeName ($([Math]::Round($size, 2)) MB)" -ForegroundColor Green
}
else {
    Write-Host "  -> FAILED: release build did not produce exe" -ForegroundColor Red
    exit 1
}

# =============================================================================
# SUMMARY
# =============================================================================
Write-Host ""
Write-Host "=======================================" -ForegroundColor White
Write-Host "         BUILD COMPLETE v$version" -ForegroundColor White
Write-Host "=======================================" -ForegroundColor White
Write-Host ""
Write-Host "  $outputExeName" -ForegroundColor Green
Write-Host "  Size: $([Math]::Round($size, 2)) MB" -ForegroundColor Gray
Write-Host ""
