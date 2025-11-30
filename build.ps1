# Extract version from Cargo.toml
$cargoContent = Get-Content "Cargo.toml" -Raw
if ($cargoContent -match 'version\s*=\s*"([^"]+)"') {
    $version = $matches[1]
} else {
    Write-Host "Failed to extract version from Cargo.toml" -ForegroundColor Red
    exit 1
}

Write-Host "Building release binary (v$version)..." -ForegroundColor Green
cargo build --release

$exePath = "target/release/screen-grounded-translator.exe"
$outputExeName = "ScreenGroundedTranslator_v$version.exe"
$outputPath = "target/release/$outputExeName"
$upxDir = "tools/upx"
$upxPath = "$upxDir/upx.exe"

# Download UPX if not present
if (-not (Test-Path $upxPath)) {
    Write-Host "Downloading UPX..." -ForegroundColor Cyan
    New-Item -ItemType Directory -Path $upxDir -Force | Out-Null
    
    $url = "https://github.com/upx/upx/releases/download/v5.0.2/upx-5.0.2-win64.zip"
    $zip = "$upxDir/upx.zip"
    
    Invoke-WebRequest -Uri $url -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath $upxDir -Force
    Move-Item "$upxDir/upx-5.0.2-win64/upx.exe" $upxPath -Force
    Remove-Item "$upxDir/upx-5.0.2-win64" -Recurse
    Remove-Item $zip
    
    Write-Host "UPX downloaded" -ForegroundColor Green
}

if (Test-Path $exePath) {
    Write-Host "Compressing with UPX..." -ForegroundColor Green
    & $upxPath --best --lzma $exePath
    
    # Rename exe to include version
    if (Test-Path $outputPath) {
        Remove-Item $outputPath
    }
    Move-Item $exePath $outputPath
    
    $size = (Get-Item $outputPath).Length / 1MB
    Write-Host "Done! Output: $outputExeName" -ForegroundColor Green
    Write-Host "Binary size: $([Math]::Round($size, 2)) MB" -ForegroundColor Green
} else {
    Write-Host "Build failed - exe not found" -ForegroundColor Red
}
