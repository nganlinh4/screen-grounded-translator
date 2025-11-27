# Build and compress the executable
Write-Host "Building release binary..." -ForegroundColor Green
cargo build --release

$exePath = "target/release/screen-grounded-translator.exe"
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
    
    $size = (Get-Item $exePath).Length / 1MB
    Write-Host "Done! Binary size: $([Math]::Round($size, 2)) MB" -ForegroundColor Green
} else {
    Write-Host "Build failed - exe not found" -ForegroundColor Red
}
