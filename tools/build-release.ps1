# Build an unsigned release: tests, notices and SBOM, the per-user installer, and a portable zip.
# Output: dist\  (installer, portable zip, SHA-256 checksums)
# Signing is a separate, approved step; nothing here publishes anything.

# Update files: set TAURI_SIGNING_PRIVATE_KEY (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD) and pass
# -UpdateBaseUrl to also produce the installer's update signature and latest.json. See docs\updates.md.
param([string]$UpdateBaseUrl = "")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "== Interface libraries"
Push-Location app
if (-not (Test-Path node_modules)) { npm install --no-audit --no-fund }
node scripts/vendor.mjs
Pop-Location

Write-Host "== Tests"
cargo test -p prm-core --release
if ($LASTEXITCODE -ne 0) { throw "Tests failed; release stopped." }

Write-Host "== Third-party notices, SBOM and license check"
python tools/third_party.py
if ($LASTEXITCODE -ne 0) { throw "A dependency license needs review; release stopped." }

Write-Host "== Installer"
Push-Location app
$updates = [bool]$env:TAURI_SIGNING_PRIVATE_KEY -and [bool]$UpdateBaseUrl
if ($updates) {
  $cfg = Join-Path $env:TEMP "prm-updater-config.json"
  [IO.File]::WriteAllText($cfg, '{"bundle":{"createUpdaterArtifacts":true}}')
  npx tauri build --config $cfg
} else {
  npx tauri build
}
if ($LASTEXITCODE -ne 0) { throw "Installer build failed." }
Pop-Location

$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force $dist | Out-Null
$setup = Get-ChildItem "target\release\bundle\nsis\*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Copy-Item $setup.FullName (Join-Path $dist "Prism-Reference-Manager-$version-setup.exe") -Force

if ($updates) {
  Write-Host "== Update signature and latest.json"
  $setupName = "Prism-Reference-Manager-$version-setup.exe"
  $sig = Get-Content ($setup.FullName + ".sig") -Raw
  Copy-Item ($setup.FullName + ".sig") (Join-Path $dist "$setupName.sig") -Force
  $manifest = [ordered]@{
    version   = $version
    notes     = "See the release notes for this version."
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = @{ "windows-x86_64" = @{ signature = $sig.Trim(); url = ($UpdateBaseUrl.TrimEnd("/") + "/" + $setupName) } }
  }
  [IO.File]::WriteAllText((Join-Path $dist "latest.json"), ($manifest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))
}

Write-Host "== Browser capture extension"
$extZip = Join-Path $dist "Prism-Reference-Manager-$version-browser-capture.zip"
if (Test-Path $extZip) { Remove-Item $extZip }
Compress-Archive -Path "extension\*" -DestinationPath $extZip

Write-Host "== Portable zip"
$stage = Join-Path $env:TEMP "prm-portable-$version"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "target\release\prism-reference-manager.exe" $stage
Copy-Item LICENSE, NOTICE, THIRD-PARTY.md $stage
Copy-Item -Recurse extension (Join-Path $stage "extension")
Set-Content (Join-Path $stage "portable.txt") "This file makes Prism Reference Manager keep its settings and default library in the data folder next to the program. Delete it to use your Windows profile instead." -Encoding utf8
$zip = Join-Path $dist "Prism-Reference-Manager-$version-portable.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip

Get-ChildItem $dist -File | Where-Object { $_.Extension -ne ".txt" } | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content (Join-Path $dist "SHA256SUMS.txt") -Encoding ascii
Get-ChildItem $dist
Write-Host "Done. These builds are unsigned."
